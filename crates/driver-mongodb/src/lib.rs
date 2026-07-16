use async_trait::async_trait;
use core_domain::{
    AppError, AppResult, ColumnDefinition, ConnectionProfile, ExplorerNode, ExplorerNodeType,
    QueryCellValue, QueryExecution, QueryResult, TableChangeSet, TableDefinition, TableRef,
};
use driver_api::{ConnectionHandle, ConnectionProvider, DatabaseDriver};
use futures::stream::TryStreamExt;
use i18n::tr;
use mongodb::bson::{doc, Bson, Document};
use mongodb::options::{ClientOptions, FindOptions, ListCollectionsOptions, SelectionCriteria};
use mongodb::Client;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

#[derive(Clone, Default)]
pub struct MongoDbDriver;

#[async_trait]
impl ConnectionProvider for MongoDbDriver {
    async fn connect(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        database: Option<&str>,
    ) -> AppResult<ConnectionHandle> {
        let uri = if let Some(ref custom_uri) = profile.connection_uri {
            custom_uri.clone()
        } else {
            let auth_db = profile.default_database.as_deref().unwrap_or("admin");
            let base = if password.is_empty() {
                format!("mongodb://{}:{}/", profile.host, profile.port)
            } else {
                format!(
                    "mongodb://{}:{}@{}:{}/",
                    urlencoding::encode(&profile.username),
                    urlencoding::encode(password),
                    profile.host,
                    profile.port,
                )
            };
            let mut params = vec![format!("authSource={}", auth_db)];
            if let Some(ref rs) = profile.replica_set {
                if !rs.is_empty() {
                    params.push(format!("replicaSet={}", rs));
                }
            }
            format!("{}?{}", base, params.join("&"))
        };
        let mut options = ClientOptions::parse(&uri)
            .await
            .map_err(map_mongo_error)?;
        options.direct_connection = Some(profile.direct_connection);
        let client = Client::with_options(options).map_err(map_mongo_error)?;
        Ok(ConnectionHandle::MongoDb {
            client,
            database: database
                .map(|s| s.to_string())
                .or_else(|| profile.default_database.clone()),
        })
    }

    async fn ping(&self, handle: &mut ConnectionHandle) -> AppResult<()> {
        let (client, db_name) = mongo_client_db(handle)?;
        let db = client.database(&db_name);
        db.run_command(doc! { "ping": 1 })
            .await
            .map_err(map_mongo_error)?;
        Ok(())
    }
}

#[async_trait]
impl DatabaseDriver for MongoDbDriver {
    async fn test_connection(
        &self,
        profile: &ConnectionProfile,
        password: &str,
    ) -> AppResult<()> {
        let mut handle = self.connect(profile, password, None).await?;
        self.ping(&mut handle).await
    }

    async fn list_roots(
        &self,
        handle: &mut ConnectionHandle,
        connection_id: &str,
    ) -> AppResult<Vec<ExplorerNode>> {
        let (client, _) = mongo_client_db(handle)?;
        let mut names = client
            .list_database_names()
            .await
            .map_err(map_mongo_error)?;
        names.sort();
        Ok(names
            .into_iter()
            .map(|db| ExplorerNode {
                id: format!("mongo-db:{connection_id}:{db}"),
                connection_id: connection_id.to_string(),
                name: db.clone(),
                node_type: ExplorerNodeType::Database,
                parent_id: None,
                database: Some(db),
                schema: None,
                expandable: true,
                loaded: false,
            })
            .collect())
    }

    async fn list_children(
        &self,
        handle: &mut ConnectionHandle,
        connection_id: &str,
        parent: &ExplorerNode,
    ) -> AppResult<Vec<ExplorerNode>> {
        if matches!(parent.node_type, ExplorerNodeType::Connection) {
            return self.list_roots(handle, connection_id).await;
        }
        let db_name = parent
            .database
            .as_ref()
            .ok_or_else(|| AppError::Validation("missing database".into()))?;
        let (client, _) = mongo_client_db(handle)?;
        let db = client.database(db_name);
        let mut names = db
            .list_collection_names()
            .await
            .map_err(map_mongo_error)?;
        names.sort();
        Ok(names
            .into_iter()
            .map(|name| ExplorerNode {
                id: format!("mongo-coll:{connection_id}:{db_name}:{name}"),
                connection_id: connection_id.to_string(),
                name: name.clone(),
                node_type: ExplorerNodeType::Table,
                parent_id: Some(parent.id.clone()),
                database: Some(db_name.clone()),
                schema: None,
                expandable: false,
                loaded: true,
            })
            .collect())
    }

    async fn load_table_definition(
        &self,
        handle: &mut ConnectionHandle,
        table: &TableRef,
    ) -> AppResult<TableDefinition> {
        let t0 = std::time::Instant::now();
        let db_name = table
            .database
            .as_deref()
            .unwrap_or("test");
        let (client, _) = mongo_client_db(handle)?;
        let db = client.database(db_name);
        let coll = db.collection::<Document>(&table.table);

        // 用 find().limit(5) 代替 $sample，避免大集合全表扫描
        let mut find_opts = mongodb::options::FindOptions::default();
        find_opts.limit = Some(5);
        let t_sample = std::time::Instant::now();
        let mut cursor = coll
            .find(doc! {})
            .with_options(find_opts)
            .await
            .map_err(map_mongo_error)?;

        let mut fields = Vec::new();
        let mut fields_set = HashSet::new();
        while let Some(doc) = cursor.try_next().await.map_err(map_mongo_error)? {
            collect_document_fields(&doc, None, &mut fields, &mut fields_set);
        }
        tracing::info!("[MongoDB] 字段采样耗时: {}ms ({}个字段)", t_sample.elapsed().as_millis(), fields.len());

        // 对类型为 null 的字段做批量查询，减少数据库往返
        let t_null = std::time::Instant::now();
        let null_fields: Vec<&str> = fields.iter()
            .filter(|f| f.data_type == "null")
            .map(|f| f.name.as_str())
            .collect();
        if !null_fields.is_empty() {
            let or_clauses: Vec<Document> = null_fields.iter()
                .map(|name| doc! { *name: { "$ne": null } })
                .collect();
            let filter = doc! { "$or": or_clauses };
            let mut opts = mongodb::options::FindOptions::default();
            opts.limit = Some(5);
            if let Ok(mut c) = coll.find(filter).with_options(opts).await {
                while let Ok(Some(doc)) = c.try_next().await {
                    for field in fields.iter_mut() {
                        if field.data_type == "null" {
                            if let Some(val) = lookup_bson_path(&doc, &field.name) {
                                let t = bson_type_name(val);
                                if t != "null" {
                                    field.data_type = t;
                                }
                            }
                        }
                    }
                }
            }
        }
        tracing::info!("[MongoDB] null字段查询耗时: {}ms", t_null.elapsed().as_millis());

        // 加载索引并生成 MongoDB 格式的 createIndex 命令
        let t_idx = std::time::Instant::now();
        let mut all_lines = vec![format!("db.createCollection(\"{}\")", table.table)];
        match coll.list_indexes().await {
            Ok(mut idx_cursor) => {
                while let Some(idx) = idx_cursor.try_next().await.map_err(map_mongo_error)? {
                    let keys = idx.keys;
                    let opts = idx.options;
                    let name = opts.as_ref().and_then(|o| o.name.as_deref()).unwrap_or("").to_string();
                    let unique = opts.as_ref().and_then(|o| o.unique).unwrap_or(false);
                    if name == "_id_" {
                        continue;
                    }
                    let keys_doc: String = keys
                        .iter()
                        .map(|(k, v)| {
                            let dir = match v.as_i32() { Some(d) => d, _ => 1 };
                            format!("\"{k}\": {dir}")
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    if unique {
                        all_lines.push(format!("db.{}.createIndex({{{keys_doc}}}, {{unique: true, name: \"{name}\"}})", table.table));
                    } else {
                        all_lines.push(format!("db.{}.createIndex({{{keys_doc}}}, {{name: \"{name}\"}})", table.table));
                    }
                }
            }
            Err(_) => {}
        }
        tracing::info!("[MongoDB] 索引加载耗时: {}ms", t_idx.elapsed().as_millis());

        tracing::info!("[MongoDB] load_table_definition 总耗时: {}ms", t0.elapsed().as_millis());
        Ok(TableDefinition {
            columns: fields,
            create_sql: Some(all_lines.join(";\n")),
            table_comment: None,
            engine: None,
            charset: None,
        })
    }

    async fn preview_table(
        &self,
        handle: &mut ConnectionHandle,
        table: &TableRef,
        limit: u32,
    ) -> AppResult<QueryResult> {
        let db_name = table.database.as_deref().unwrap_or("test");
        let (client, _) = mongo_client_db(handle)?;
        let db = client.database(db_name);
        let coll = db.collection::<Document>(&table.table);

        let start = Instant::now();
        let opts = FindOptions::builder()
            .limit(Some(limit as i64))
            .batch_size(Some(limit.max(1024)))
            .build();
        let mut cursor = coll
            .find(doc! {})
            .with_options(opts)
            .await
            .map_err(map_mongo_error)?;

        let (columns, rows, mongo_types) = collect_cursor(&mut cursor).await?;

        Ok(QueryResult {
            columns,
            rows,
            affected_rows: None,
            elapsed_ms: start.elapsed().as_millis(),
            message: None,
            mongo_types,
        })
    }

    async fn execute_sql(
        &self,
        handle: &mut ConnectionHandle,
        execution: QueryExecution,
    ) -> AppResult<QueryResult> {
        let (client, cur_db) = mongo_client_db(handle)?;
        let db_name = execution.database.as_deref().unwrap_or(&cur_db);
        let sql = execution.sql.trim();
        let start = Instant::now();

        let result = execute_mongo_command(client, db_name, sql).await?;
        let elapsed = start.elapsed().as_millis();
        tracing::info!("[MongoDB] execute_sql 总耗时: {}ms ({}行)", elapsed, result.1.len());

        Ok(QueryResult {
            columns: result.0,
            rows: result.1,
            affected_rows: result.2,
            elapsed_ms: elapsed,
            message: result.3,
            mongo_types: result.4,
        })
    }

    async fn apply_table_changes(
        &self,
        _handle: &mut ConnectionHandle,
        _changes: TableChangeSet,
    ) -> AppResult<QueryResult> {
        Err(AppError::Unsupported(
            tr!("MongoDB 表格编辑将在后续迭代中补全").to_string(),
        ))
    }

    async fn create_database(
        &self,
        handle: &mut ConnectionHandle,
        name: &str,
        _charset: Option<&str>,
        _collation: Option<&str>,
    ) -> AppResult<()> {
        let (client, _) = mongo_client_db(handle)?;
        let db = client.database(name);
        db.create_collection("__placeholder")
            .await
            .map_err(map_mongo_error)?;
        Ok(())
    }

    async fn rename_database(
        &self,
        _handle: &mut ConnectionHandle,
        _old_name: &str,
        _new_name: &str,
    ) -> AppResult<()> {
        Err(AppError::Unsupported(
            tr!("MongoDB 不支持重命名数据库").to_string(),
        ))
    }

    async fn drop_database(
        &self,
        handle: &mut ConnectionHandle,
        name: &str,
    ) -> AppResult<()> {
        let (client, _) = mongo_client_db(handle)?;
        client
            .database(name)
            .drop()
            .await
            .map_err(map_mongo_error)?;
        Ok(())
    }

    async fn create_schema(
        &self,
        _handle: &mut ConnectionHandle,
        _database: &str,
        _name: &str,
    ) -> AppResult<()> {
        Err(AppError::Unsupported(
            tr!("MongoDB 不支持 Schema").to_string(),
        ))
    }

    async fn rename_schema(
        &self,
        _handle: &mut ConnectionHandle,
        _database: &str,
        _old_name: &str,
        _new_name: &str,
    ) -> AppResult<()> {
        Err(AppError::Unsupported(
            tr!("MongoDB 不支持 Schema").to_string(),
        ))
    }

    async fn drop_schema(
        &self,
        _handle: &mut ConnectionHandle,
        _database: &str,
        _name: &str,
    ) -> AppResult<()> {
        Err(AppError::Unsupported(
            tr!("MongoDB 不支持 Schema").to_string(),
        ))
    }

    async fn rename_table(
        &self,
        handle: &mut ConnectionHandle,
        database: &str,
        _schema: Option<&str>,
        old_name: &str,
        new_name: &str,
    ) -> AppResult<()> {
        let (client, _) = mongo_client_db(handle)?;
        let admin_db = client.database("admin");
        admin_db.run_command(doc! {
            "renameCollection": format!("{}.{}", database, old_name),
            "to": format!("{}.{}", database, new_name)
        })
        .await
        .map_err(map_mongo_error)?;
        Ok(())
    }

    async fn dump_table_all_data(
        &self,
        handle: &mut ConnectionHandle,
        table: &TableRef,
    ) -> AppResult<QueryResult> {
        let db_name = table.database.as_deref().unwrap_or("test");
        let (client, _) = mongo_client_db(handle)?;
        let db = client.database(db_name);
        let coll = db.collection::<Document>(&table.table);

        let start = Instant::now();
        let mut cursor = coll.find(doc! {}).await.map_err(map_mongo_error)?;

        let (columns, rows, mongo_types) = collect_cursor(&mut cursor).await?;

        Ok(QueryResult {
            columns,
            rows,
            affected_rows: None,
            elapsed_ms: start.elapsed().as_millis(),
            message: None,
            mongo_types,
        })
    }

    async fn load_tables_summary(
        &self,
        handle: &mut ConnectionHandle,
        database: &str,
        _schema: Option<&str>,
    ) -> AppResult<Vec<driver_api::TableSummary>> {
        let (client, _) = mongo_client_db(handle)?;
        let client = client.clone(); // cheap — 内部是 Arc
        let db = client.database(database);

        // 用 listCollections 一条命令获取集合列表（秒返回，不获取统计信息）
        let mut specs: Vec<_> = db.list_collections().await.map_err(map_mongo_error)?
            .try_collect().await.map_err(map_mongo_error)?;
        specs.sort_by(|a, b| a.name.cmp(&b.name));
        let mut summaries = Vec::new();
        for spec in specs {
            let table_type = match spec.collection_type {
                mongodb::results::CollectionType::View => "VIEW",
                _ => "COLLECTION",
            }.into();
            summaries.push(driver_api::TableSummary {
                name: spec.name,
                table_type,
                row_count: None,
                total_size: None,
                data_size: None,
                index_size: None,
                engine: None,
                collation: None,
                primary_keys: vec!["_id".into()],
                comment: None,
                create_time: None,
            });
        }
        Ok(summaries)
    }

    async fn load_collection_stats(
        &self,
        handle: &mut ConnectionHandle,
        database: &str,
        collection: &str,
    ) -> AppResult<Option<(Option<i64>, Option<i64>, Option<i64>, Option<i64>)>> {
        let (client, _) = mongo_client_db(handle)?;
        let client = client.clone();
        let db = client.database(database);
        let stats = db.run_command(doc! { "collStats": collection }).await.map_err(map_mongo_error)?;

        fn to_i64(doc: &Document, key: &str) -> Option<i64> {
            match doc.get(key)? {
                Bson::Int32(v) => Some(*v as i64),
                Bson::Int64(v) => Some(*v),
                Bson::Double(v) => Some(*v as i64),
                _ => None,
            }
        }

        let count = to_i64(&stats, "count");
        let size = to_i64(&stats, "size");
        let storage = to_i64(&stats, "storageSize");
        let idx_size = to_i64(&stats, "totalIndexSize");
        let data_size = size.or(storage);
        let total = match (data_size, idx_size) {
            (Some(s), Some(i)) => Some(s + i),
            (s, i) => s.or(i),
        };
        Ok(Some((count, data_size, idx_size, total)))
    }
}

fn collect_document_fields(
    doc: &Document,
    prefix: Option<&str>,
    fields: &mut Vec<ColumnDefinition>,
    fields_set: &mut HashSet<String>,
) {
    for (key, value) in doc {
        let path = match prefix {
            Some(prefix) if !prefix.is_empty() => format!("{prefix}.{key}"),
            _ => key.clone(),
        };
        if fields_set.insert(path.clone()) {
            fields.push(ColumnDefinition {
                name: path.clone(),
                data_type: bson_type_name(value),
                nullable: true,
                primary_key: path == "_id",
                unique: path == "_id",
                auto_increment: false,
                on_update_current_timestamp: false,
                default_value: None,
                comment: None,
            });
        }
        match value {
            Bson::Document(nested) => collect_document_fields(nested, Some(&path), fields, fields_set),
            Bson::Array(items) => {
                for item in items {
                    if let Bson::Document(nested) = item {
                        collect_document_fields(nested, Some(&path), fields, fields_set);
                    }
                }
            }
            _ => {}
        }
    }
}

fn lookup_bson_path<'a>(doc: &'a Document, path: &str) -> Option<&'a Bson> {
    fn descend<'a>(value: &'a Bson, segments: &[&str]) -> Option<&'a Bson> {
        if segments.is_empty() {
            return Some(value);
        }
        let (head, tail) = segments.split_first()?;
        match value {
            Bson::Document(document) => descend(document.get(*head)?, tail),
            Bson::Array(items) => items.iter().find_map(|item| match item {
                Bson::Document(document) => descend(document.get(*head)?, tail),
                _ => None,
            }),
            _ => None,
        }
    }

    let mut segments = path.split('.');
    let first = segments.next()?;
    let first_value = doc.get(first)?;
    let rest: Vec<_> = segments.collect();
    descend(first_value, &rest)
}

// ── Mongo Shell 命令解析与执行 ──

async fn execute_mongo_command(
    client: &Client,
    db_name: &str,
    input: &str,
) -> AppResult<(Vec<String>, Vec<BTreeMap<String, QueryCellValue>>, Option<u64>, Option<String>, HashMap<(usize, String), MongoValue>)> {
    let t_cmd = std::time::Instant::now();
    let input = strip_comments(input);
    let lower = input.to_lowercase();

    // show dbs
    if lower == "show dbs" || lower == "show databases" {
        let names = client
            .list_database_names()
            .await
            .map_err(map_mongo_error)?;
        let columns = vec!["name".to_string()];
        let rows: Vec<_> = names
            .into_iter()
            .map(|name| {
                let mut m = BTreeMap::new();
                m.insert("name".to_string(), QueryCellValue::Text(name));
                m
            })
            .collect();
        return Ok((columns, rows, None, None, HashMap::new()));
    }

    // show collections / show tables
    if lower == "show collections" || lower == "show tables" {
        let db = client.database(db_name);
        let names = db
            .list_collection_names()
            .await
            .map_err(map_mongo_error)?;
        let columns = vec!["name".to_string()];
        let rows: Vec<_> = names
            .into_iter()
            .map(|name| {
                let mut m = BTreeMap::new();
                m.insert("name".to_string(), QueryCellValue::Text(name));
                m
            })
            .collect();
        return Ok((columns, rows, None, None, HashMap::new()));
    }

    // use <database>
    if lower.starts_with("use ") {
        let target = input[4..].trim();
        // 不返回结果，仅提示
        return Ok((
            Vec::new(),
            Vec::new(),
            None,
            Some(tr!("已切换到数据库: {}").replace("{}", target)),
            HashMap::new(),
        ));
    }

    // db.<collection>.<method>(...)
    if let Some(parsed) = parse_db_command(&input) {
        let db = client.database(db_name);
        let coll = db.collection::<Document>(&parsed.collection);

        // EXPLAIN: 将命令包装为 { explain: { <cmd> } }
        if parsed.explain {
            let cmd_doc = match parsed.method.as_str() {
                "find" => {
                    let (filter, projection) = parse_find_args(&parsed.args)?;
                    let mut doc = doc! { "find": &parsed.collection, "filter": filter };
                    if let Some(proj) = projection { doc.insert("projection", proj); }
                    if let Some(limit) = parsed.limit { doc.insert("limit", limit); }
                    if let Some(skip) = parsed.skip { doc.insert("skip", skip as i64); }
                    if let Some(ref sort) = parsed.sort { doc.insert("sort", sort.clone()); }
                    doc
                }
                "aggregate" => {
                    let pipeline = parse_pipeline(&parsed.args)?;
                    doc! { "aggregate": &parsed.collection, "pipeline": pipeline, "cursor": {} }
                }
                other => return Err(AppError::Query(tr!("EXPLAIN 不支持的方法: {}").replace("{}", other))),
            };
            let explain_doc = db.run_command(doc! { "explain": cmd_doc }).await.map_err(map_mongo_error)?;
            let json = serde_json::to_string_pretty(&explain_doc).unwrap_or_else(|_| format!("{:?}", explain_doc));
            let columns = vec!["executionPlan".to_string()];
            let mut row = BTreeMap::new();
            row.insert("executionPlan".to_string(), QueryCellValue::Text(json));
            return Ok((columns, vec![row], None, None, HashMap::new()));
        }

        return match parsed.method.as_str() {
            "find" => {
                let (filter, projection) = parse_find_args(&parsed.args)?;
                let mut opts = mongodb::options::FindOptions::default();
                opts.projection = projection;
                opts.limit = parsed.limit;
                opts.skip = parsed.skip;
                opts.sort = parsed.sort;
                let t0 = std::time::Instant::now();
                let mut cursor = coll
                    .find(filter)
                    .with_options(opts)
                    .await
                    .map_err(map_mongo_error)?;
                tracing::info!("[MongoDB] find 执行耗时: {}ms", t0.elapsed().as_millis());
                let (columns, rows, mongo_types) = collect_cursor(&mut cursor).await?;
                let msg = Some(tr!("查询完成，返回 {} 条记录").replace("{}", &rows.len().to_string()));
                Ok((columns, rows, None, msg, mongo_types))
            }
            "findOne" => {
                let (filter, projection) = parse_find_args(&parsed.args)?;
                let mut opts = mongodb::options::FindOneOptions::default();
                opts.projection = projection;
                let result = coll
                    .find_one(filter)
                    .with_options(opts)
                    .await
                    .map_err(map_mongo_error)?;
                match result {
                    Some(doc) => {
                        let mut columns = Vec::new();
                        let mut row = BTreeMap::new();
                        for (key, value) in &doc {
                            columns.push(key.clone());
                            row.insert(key.clone(), bson_to_cell(Some(value)));
                        }
                        Ok((columns, vec![row], None, None, HashMap::new()))
                    }
                    None => Ok((Vec::new(), Vec::new(), None, Some("null".to_string()), HashMap::new())),
                }
            }
            "aggregate" => {
                let pipeline = parse_pipeline(&parsed.args)?;
                let t0 = std::time::Instant::now();
                let mut cursor = coll
                    .aggregate(pipeline)
                    .await
                    .map_err(map_mongo_error)?;
                tracing::info!("[MongoDB] aggregate 执行耗时: {}ms", t0.elapsed().as_millis());
                let (columns, rows, mongo_types) = collect_cursor(&mut cursor).await?;
                let msg = Some(tr!("查询完成，返回 {} 条记录").replace("{}", &rows.len().to_string()));
                Ok((columns, rows, None, msg, mongo_types))
            }
            "count" | "countDocuments" => {
                let filter = parse_single_doc(&parsed.args).unwrap_or_default();
                let count = coll
                    .count_documents(filter)
                    .await
                    .map_err(map_mongo_error)?;
                let columns = vec!["count".to_string()];
                let mut row = BTreeMap::new();
                row.insert(
                    "count".to_string(),
                    QueryCellValue::Text(count.to_string()),
                );
                Ok((columns, vec![row], None, None, HashMap::new()))
            }
            "distinct" => {
                // distinct("field", filter)
                let field = parse_string_arg(&parsed.args)
                    .ok_or_else(|| AppError::Validation("distinct requires a field name".into()))?;
                let filter = parse_second_doc_arg(&parsed.args).unwrap_or_default();
                let result = coll
                    .distinct(field, filter)
                    .await
                    .map_err(map_mongo_error)?;
                let columns = vec!["value".to_string()];
                let rows: Vec<_> = result
                    .into_iter()
                    .map(|b| {
                        let mut m = BTreeMap::new();
                        m.insert("value".to_string(), bson_to_cell(Some(&b)));
                        m
                    })
                    .collect();
                Ok((columns, rows, None, None, HashMap::new()))
            }
            "drop" => {
                coll.drop().await.map_err(map_mongo_error)?;
                Ok((
                    Vec::new(),
                    Vec::new(),
                    None,
                    Some(tr!("已删除集合: {}").replace("{}", &parsed.collection)),
                    HashMap::new(),
                ))
            }
            "insertOne" => {
                let doc = parse_single_doc(&parsed.args)
                    .ok_or_else(|| AppError::Validation(tr!("insertOne 需要一个文档参数").into()))?;
                let result = coll
                    .insert_one(doc)
                    .await
                    .map_err(map_mongo_error)?;
                Ok((
                    Vec::new(),
                    Vec::new(),
                    Some(1),
                    Some(tr!("已插入 1 条记录，_id: {}").replace("{}", &result.inserted_id.to_string())),
                    HashMap::new(),
                ))
            }
            "insertMany" => {
                let trimmed = parsed.args.trim();
                let inner = trimmed
                    .strip_prefix('[')
                    .and_then(|s| s.strip_suffix(']'))
                    .unwrap_or(trimmed);
                let parts = split_top_level_args(inner);
                let mut docs = Vec::new();
                for part in parts {
                    docs.push(parse_jsonish_doc(part)?);
                }
                let count = docs.len();
                let result = coll
                    .insert_many(docs)
                    .await
                    .map_err(map_mongo_error)?;
                Ok((
                    Vec::new(),
                    Vec::new(),
                    Some(result.inserted_ids.len() as u64),
                    Some(tr!("已插入 {} 条记录").replace("{}", &count.to_string())),
                    HashMap::new(),
                ))
            }
            "deleteOne" => {
                let filter = parse_single_doc(&parsed.args).unwrap_or_default();
                let result = coll
                    .delete_one(filter)
                    .await
                    .map_err(map_mongo_error)?;
                Ok((
                    Vec::new(),
                    Vec::new(),
                    Some(result.deleted_count),
                    Some(tr!("已删除 {} 条记录").replace("{}", &result.deleted_count.to_string())),
                    HashMap::new(),
                ))
            }
            "deleteMany" => {
                let filter = parse_single_doc(&parsed.args).unwrap_or_default();
                let result = coll
                    .delete_many(filter)
                    .await
                    .map_err(map_mongo_error)?;
                Ok((
                    Vec::new(),
                    Vec::new(),
                    Some(result.deleted_count),
                    Some(tr!("已删除 {} 条记录").replace("{}", &result.deleted_count.to_string())),
                    HashMap::new(),
                ))
            }
            "updateOne" => {
                let parts: Vec<_> = split_top_level_args(&parsed.args);
                if parts.len() < 2 {
                    return Err(AppError::Validation(tr!("updateOne 需要 filter 和 update 两个参数").into()));
                }
                let filter = parse_jsonish_doc(parts[0])?;
                let update = parse_jsonish_doc(parts[1])?;
                let result = coll
                    .update_one(filter, update)
                    .await
                    .map_err(map_mongo_error)?;
                Ok((
                    Vec::new(),
                    Vec::new(),
                    Some(result.modified_count),
                    Some(tr!("已更新 {} 条记录").replace("{}", &result.modified_count.to_string())),
                    HashMap::new(),
                ))
            }
            "updateMany" => {
                let parts: Vec<_> = split_top_level_args(&parsed.args);
                if parts.len() < 2 {
                    return Err(AppError::Validation(tr!("updateMany 需要 filter 和 update 两个参数").into()));
                }
                let filter = parse_jsonish_doc(parts[0])?;
                let update = parse_jsonish_doc(parts[1])?;
                let result = coll
                    .update_many(filter, update)
                    .await
                    .map_err(map_mongo_error)?;
                Ok((
                    Vec::new(),
                    Vec::new(),
                    Some(result.modified_count),
                    Some(tr!("已更新 {} 条记录").replace("{}", &result.modified_count.to_string())),
                    HashMap::new(),
                ))
            }
            "createCollection" => {
                let db = client.database(db_name);
                db.create_collection(&parsed.collection)
                    .await
                    .map_err(map_mongo_error)?;
                Ok((
                    Vec::new(),
                    Vec::new(),
                    None,
                    Some(tr!("已创建集合: {}").replace("{}", &parsed.collection)),
                    HashMap::new(),
                ))
            }
            _ => Err(AppError::Query(format!(
                "unsupported method: {}",
                parsed.method
            ))),
        };
    }

    // db.createCollection("name") — 不遵循 db.<coll>.<method>() 模式
    let lower_trimmed = input.trim();
    if lower_trimmed.starts_with("db.createCollection(") {
        let inner = &lower_trimmed["db.createCollection(".len()..];
        let name_part = inner.trim_start().trim_end_matches(')').trim();
        let name = name_part.trim_matches(|c| c == '"' || c == '\'');
        if name.is_empty() {
            return Err(AppError::Validation(tr!("createCollection 需要集合名称").into()));
        }
        let db = client.database(db_name);
        db.create_collection(name)
            .await
            .map_err(map_mongo_error)?;
        return Ok((
            Vec::new(),
            Vec::new(),
            None,
            Some(tr!("已创建集合: {}").replace("{}", name)),
            HashMap::new(),
        ));
    }

    Err(AppError::Query(tr!(
        "无法解析 MongoDB 命令。支持的语法: show dbs, show collections, db.<collection>.find(), db.<collection>.aggregate([])"
    )
    .to_string()))
}

// ── Shell 命令解析 ──

struct DbCommand {
    collection: String,
    method: String,
    args: String,
    limit: Option<i64>,
    skip: Option<u64>,
    sort: Option<Document>,
    explain: bool,
}

fn parse_db_command(input: &str) -> Option<DbCommand> {
    let trimmed = input.trim();
    // 要求以 "db." 开头
    if !trimmed.starts_with("db.") {
        return None;
    }
    let rest = &trimmed[3..];

    // 找到 collection 名（到第一个 '.' 或 '('）
    let dot_pos = rest.find('.')?;
    let coll_name = rest[..dot_pos].trim().to_string();
    if coll_name.is_empty() {
        return None;
    }

    let after_dot = rest[dot_pos + 1..].trim();

    // 解析 method(...)
    let paren_start = after_dot.find('(')?;
    let method = after_dot[..paren_start].trim().to_string();

    // 找到匹配的右括号
    let args_start = paren_start + 1;
    let args = find_balanced_parens(&after_dot[args_start..])?;

    // 解析方法链 .limit(N) / .skip(N) / .sort({...}) / .explain()
    let mut remaining = &after_dot[args_start + args.len() + 1..];
    let mut limit = None;
    let mut skip = None;
    let mut sort = None;
    let mut explain = false;
    while remaining.starts_with('.') {
        remaining = &remaining[1..];
        let chain_paren = remaining.find('(')?;
        let chain_method = remaining[..chain_paren].trim();
        let chain_args_start = chain_paren + 1;
        let chain_args = find_balanced_parens(&remaining[chain_args_start..])?;
        match chain_method {
            "limit" => limit = chain_args.trim().parse().ok(),
            "skip" => skip = chain_args.trim().parse().ok(),
            "sort" => {
                sort = serde_json::from_str::<serde_json::Value>(chain_args.trim())
                    .ok()
                    .and_then(|v| mongodb::bson::to_bson(&v).ok())
                    .and_then(|b| b.as_document().cloned());
            }
            "explain" => explain = true,
            _ => {}
        }
        remaining = &remaining[chain_args_start + chain_args.len() + 1..];
    }

    Some(DbCommand {
        collection: coll_name,
        method,
        args: args.to_string(),
        limit,
        skip,
        sort,
        explain,
    })
}

/// 找到匹配的右括号，返回括号内的内容（不含括号）
/// 跳过引号内的字符，避免字符串中的括号干扰深度计算
fn find_balanced_parens(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut depth = 0i32;
    let mut i = 0;
    while i < len {
        match bytes[i] {
            b'"' | b'\'' => {
                let q = bytes[i];
                i += 1;
                while i < len {
                    if bytes[i] == b'\\' && i + 1 < len {
                        i += 2;
                    } else if bytes[i] == q {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            b'(' => depth += 1,
            b')' => {
                if depth == 0 {
                    return Some(&s[..i]);
                }
                depth -= 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

// ── 参数解析 ──

fn parse_find_args(args: &str) -> AppResult<(Document, Option<Document>)> {
    let trimmed = args.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return Ok((doc! {}, None));
    }

    let mut parts = split_top_level_args(trimmed);
    let filter = if !parts.is_empty() {
        parse_jsonish_doc(parts.remove(0))?
    } else {
        doc! {}
    };
    let projection = if !parts.is_empty() {
        Some(parse_jsonish_doc(parts.remove(0))?)
    } else {
        None
    };
    Ok((filter, projection))
}

fn parse_pipeline(args: &str) -> AppResult<Vec<Document>> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    // 应该是一个数组 [{...}, {...}]
    let trimmed = trimmed.trim_start_matches('[').trim_end_matches(']').trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let parts = split_top_level_args(trimmed);
    let mut pipeline = Vec::new();
    for part in parts {
        pipeline.push(parse_jsonish_doc(part)?);
    }
    Ok(pipeline)
}

fn parse_single_doc(args: &str) -> Option<Document> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Some(doc! {});
    }
    // 取第一个参数
    let first = split_top_level_args(trimmed).into_iter().next()?;
    parse_jsonish_doc(first).ok()
}

fn parse_second_doc_arg(args: &str) -> Option<Document> {
    let parts = split_top_level_args(args.trim());
    if parts.len() >= 2 {
        parse_jsonish_doc(parts[1]).ok()
    } else {
        Some(doc! {})
    }
}

fn parse_string_arg(args: &str) -> Option<String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return None;
    }
    let first = split_top_level_args(trimmed).into_iter().next()?;
    let first = first.trim();
    // 去掉引号
    if first.starts_with('"') && first.ends_with('"') {
        Some(first[1..first.len() - 1].to_string())
    } else if first.starts_with('\'') && first.ends_with('\'') {
        Some(first[1..first.len() - 1].to_string())
    } else {
        Some(first.to_string())
    }
}

/// 按逗号分割顶层参数（括号/大括号/中括号内的逗号不分割）
fn split_top_level_args(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut string_char = b'\0';

    for (i, b) in s.bytes().enumerate() {
        if in_string {
            if b == string_char && s.as_bytes().get(i.wrapping_sub(1)) != Some(&b'\\') {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => {
                in_string = true;
                string_char = b;
            }
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = s[start..].trim();
    if !last.is_empty() {
        parts.push(last);
    }
    parts
}

/// 解析类 JSON（支持 Mongo Shell 语法：单引号、无引号键、尾逗号、ObjectId(...)、ISODate(...) 等）
fn parse_jsonish_doc(input: &str) -> AppResult<Document> {
    let preprocessed = preprocess_mongo_json(input);
    let value: serde_json::Value = serde_json::from_str(&preprocessed)
        .map_err(|e| AppError::Query(tr!("JSON 解析错误: {}").replace("{}", &e.to_string())))?;
    match json_value_to_bson(value) {
        Bson::Document(doc) => Ok(doc),
        other => Ok(doc! { "_value": other }),
    }
}

/// 将 serde_json::Value 转换为 Bson，递归处理扩展 JSON 标记
fn json_value_to_bson(value: serde_json::Value) -> Bson {
    match value {
        serde_json::Value::Null => Bson::Null,
        serde_json::Value::Bool(b) => Bson::Boolean(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Bson::Int64(i)
            } else if let Some(f) = n.as_f64() {
                Bson::Double(f)
            } else {
                Bson::String(n.to_string())
            }
        }
        serde_json::Value::String(s) => Bson::String(s),
        serde_json::Value::Array(arr) => Bson::Array(arr.into_iter().map(json_value_to_bson).collect()),
        serde_json::Value::Object(map) => {
            if map.len() == 1 {
                if let Some((key, val)) = map.iter().next() {
                    let key_str = key.as_str();
                    if let serde_json::Value::String(s) = val {
                        match key_str {
                            "$date" => {
                                // RFC 3339 / ISO 8601
                                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                                    let sys: std::time::SystemTime = dt.with_timezone(&chrono::Utc).into();
                                    return Bson::DateTime(sys.into());
                                }
                                // 回退：尝试常见非 RFC3339 格式
                                for fmt in &[
                                    "%Y-%m-%d %H:%M:%S%.f",
                                    "%Y-%m-%d %H:%M:%S",
                                    "%Y-%m-%dT%H:%M:%S%.f",
                                    "%Y-%m-%dT%H:%M:%S",
                                    "%Y-%m-%d",
                                ] {
                                    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
                                        let dt = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(ndt, chrono::Utc);
                                        let sys: std::time::SystemTime = dt.into();
                                        return Bson::DateTime(sys.into());
                                    }
                                    if let Ok(nd) = chrono::NaiveDate::parse_from_str(s, fmt) {
                                        let ndt = nd.and_hms_opt(0, 0, 0).unwrap();
                                        let dt = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(ndt, chrono::Utc);
                                        let sys: std::time::SystemTime = dt.into();
                                        return Bson::DateTime(sys.into());
                                    }
                                }
                            }
                            "$numberDecimal" => {
                                if let Ok(d) = s.parse::<mongodb::bson::Decimal128>() {
                                    return Bson::Decimal128(d);
                                }
                            }
                            "$oid" => {
                                if let Ok(oid) = mongodb::bson::oid::ObjectId::parse_str(s) {
                                    return Bson::ObjectId(oid);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            let mut doc = Document::new();
            for (k, v) in map {
                doc.insert(k, json_value_to_bson(v));
            }
            Bson::Document(doc)
        }
    }
}

/// 去除 // 行注释和 /* */ 块注释，保留字符串内的内容
fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' || ch == '\'' {
            let q = ch;
            out.push(ch);
            while let Some(sc) = chars.next() {
                out.push(sc);
                if sc == '\\' {
                    if let Some(ec) = chars.next() { out.push(ec); }
                } else if sc == q {
                    break;
                }
            }
        } else if ch == '/' {
            match chars.peek() {
                Some('/') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\n' { out.push('\n'); break; }
                    }
                }
                Some('*') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '*' && chars.peek() == Some(&'/') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => out.push(ch),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// 将 Mongo Shell 风格的 JSON 预处理为标准 JSON
/// 将 MongoDB Shell 语法转换为 JSON
/// 单遍处理：字符串标准化 + 无引号键名加引号 + 函数调用转换 + 尾逗号
fn preprocess_mongo_json(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    let mut ci = input.char_indices().peekable();
    let mut in_string = false;

    while let Some((byte_pos, ch)) = ci.next() {
        let remaining = &input[byte_pos..];

        // 行注释
        if !in_string && remaining.starts_with("//") {
            while let Some((_, c)) = ci.next() {
                if c == '\n' { break; }
            }
            continue;
        }

        // 块注释
        if !in_string && remaining.starts_with("/*") {
            ci.next(); // skip second '*'
            while let Some((_, c)) = ci.next() {
                if c == '*' {
                    if let Some((_, '/')) = ci.peek() {
                        ci.next();
                        break;
                    }
                }
            }
            continue;
        }

        // 双引号字符串
        if ch == '"' {
            if in_string {
                out.push('"');
                in_string = false;
            } else {
                out.push('"');
                in_string = true;
                while let Some((_, sc)) = ci.next() {
                    if sc == '\\' {
                        out.push(sc);
                        if let Some((_, ec)) = ci.next() {
                            out.push(ec);
                        }
                    } else if sc == '"' {
                        out.push('"');
                        in_string = false;
                        break;
                    } else {
                        out.push(sc);
                    }
                }
            }
            continue;
        }

        // 单引号字符串 → 双引号
        if !in_string && ch == '\'' {
            out.push('"');
            while let Some((_, sc)) = ci.next() {
                if sc == '\\' {
                    out.push(sc);
                    if let Some((_, ec)) = ci.next() {
                        out.push(ec);
                    }
                } else if sc == '\'' {
                    out.push('"');
                    break;
                } else if sc == '"' {
                    out.push('\\');
                    out.push('"');
                } else {
                    out.push(sc);
                }
            }
            continue;
        }

        if in_string {
            out.push(ch);
            continue;
        }

        // 函数调用转换（使用 remaining 匹配，然后消费迭代器）
        if remaining.starts_with("new Date(") {
            for _ in 0..8 { ci.next(); } // 跳过 "new Date" (已消费 'n' 共 9 字符)
            while let Some((_, c)) = ci.next() {
                if c == ')' { break; }
            }
            let now = chrono::Utc::now().to_rfc3339();
            out.push_str(&format!("{{\"$date\":\"{}\"}}", now));
            continue;
        }
        if remaining.starts_with("NumberDecimal(") {
            for _ in 0..13 { ci.next(); } // 跳过 "NumberDecimal" (已消费 'N' 共 14 字符)
            if let Some((val, consumed)) = extract_quoted_arg(remaining[14..].as_bytes()) {
                out.push_str(&format!("{{\"$numberDecimal\":\"{}\"}}", val));
                for _ in 0..consumed { ci.next(); }
            }
            while let Some((_, c)) = ci.next() {
                if c == ')' { break; }
            }
            continue;
        }
        if remaining.starts_with("ObjectId(") {
            for _ in 0..8 { ci.next(); } // 跳过 "bjectId(" (已消费 'O' 共 9 字符)
            if let Some((val, consumed)) = extract_quoted_arg(remaining[9..].as_bytes()) {
                out.push_str(&format!("{{\"$oid\":\"{}\"}}", val));
                for _ in 0..consumed { ci.next(); }
            }
            while let Some((_, c)) = ci.next() {
                if c == ')' { break; }
            }
            continue;
        }
        if remaining.starts_with("ISODate(") {
            for _ in 0..7 { ci.next(); } // 跳过 "SODate(" (已消费 'I' 共 8 字符)
            if let Some((val, consumed)) = extract_quoted_arg(remaining[8..].as_bytes()) {
                out.push_str(&format!("{{\"$date\":\"{}\"}}", val));
                for _ in 0..consumed { ci.next(); }
            }
            while let Some((_, c)) = ci.next() {
                if c == ')' { break; }
            }
            continue;
        }
        if remaining.starts_with("Date(") {
            for _ in 0..4 { ci.next(); } // 跳过 "Date" (已消费 'D' 共 5 字符)
            if let Some((val, consumed)) = extract_quoted_arg(remaining[5..].as_bytes()) {
                out.push_str(&format!("{{\"$date\":\"{}\"}}", val));
                for _ in 0..consumed { ci.next(); }
            }
            while let Some((_, c)) = ci.next() {
                if c == ')' { break; }
            }
            continue;
        }

        // { 和 [ 后的无引号键名检测
        if ch == '{' || ch == '[' {
            out.push(ch);
            // 跳过空白（含换行）
            while let Some(&(_, nc)) = ci.peek() {
                if nc == ' ' || nc == '\n' || nc == '\r' || nc == '\t' {
                    out.push(nc);
                    ci.next();
                } else {
                    break;
                }
            }
            // 检查是否是标识符开头 → 读取完整标识符 + 冒号判断
            try_read_unquoted_key(&input, &mut ci, &mut out);
            continue;
        }

        // 逗号处理
        if ch == ',' {
            let rest_after = input[byte_pos + 1..].trim_start();
            if rest_after.starts_with('}') || rest_after.starts_with(']') {
                continue; // 尾逗号，跳过
            }
            out.push(',');
            // 跳过空白
            while let Some(&(_, nc)) = ci.peek() {
                if nc == ' ' || nc == '\n' || nc == '\r' || nc == '\t' {
                    out.push(nc);
                    ci.next();
                } else {
                    break;
                }
            }
            // 尝试无引号键名
            try_read_unquoted_key(&input, &mut ci, &mut out);
            continue;
        }

        out.push(ch);
    }

    out
}

/// 在 { , [ 后尝试读取无引号键名并加引号
/// 如果不是键名（没有后跟冒号），不消费任何字符
fn try_read_unquoted_key(input: &str, ci: &mut std::iter::Peekable<std::str::CharIndices<'_>>, out: &mut String) {
    if let Some(&(_, nc)) = ci.peek() {
        if nc.is_ascii_alphabetic() || nc == '_' || nc == '$' {
            let start_pos = ci.peek().map(|&(idx, _)| idx).unwrap_or(input.len());
            let rest = &input[start_pos..];
            let ident_len: usize = rest.chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
                .map(|c| c.len_utf8())
                .sum();
            if ident_len > 0 {
                let ident = &rest[..ident_len];
                let after_ident = &rest[ident_len..];
                if after_ident.trim_start().starts_with(':') {
                    // 是键名，加引号
                    out.push('"');
                    out.push_str(ident);
                    out.push('"');
                    for _ in 0..ident_len { ci.next(); }
                }
                // 如果不是键名，不消费字符，让主循环逐字符处理
            }
        }
    }
}


/// 从字节序列中提取带引号的参数（支持单/双引号），返回 (值, 消耗的字节数含引号，不含外层括号)
fn extract_quoted_arg(bytes: &[u8]) -> Option<(String, usize)> {
    if bytes.is_empty() {
        return None;
    }
    let quote = if bytes[0] == b'"' || bytes[0] == b'\'' {
        bytes[0]
    } else {
        return None;
    };
    let mut i = 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
        } else if bytes[i] == quote {
            let val = String::from_utf8_lossy(&bytes[1..i]).to_string();
            let consumed = i + 1; // 引号内容 + 结束引号（不含外层括号）
            return Some((val, consumed));
        } else {
            i += 1;
        }
    }
    None
}

// ── 辅助函数 ──

fn mongo_client_db(handle: &ConnectionHandle) -> AppResult<(&Client, String)> {
    match handle {
        ConnectionHandle::MongoDb { client, database } => {
            let db = database.clone().unwrap_or_else(|| "test".to_string());
            Ok((client, db))
        }
        _ => Err(AppError::Validation("expected mongodb handle".into())),
    }
}

use core_domain::MongoValue;

async fn collect_cursor(
    cursor: &mut mongodb::Cursor<Document>,
) -> AppResult<(Vec<String>, Vec<BTreeMap<String, QueryCellValue>>, HashMap<(usize, String), MongoValue>)> {
    let t0 = std::time::Instant::now();
    let mut columns = Vec::new();
    let mut columns_set = HashSet::new();
    let mut rows = Vec::new();
    let mut mongo_types = HashMap::new();
    let mut t_fetch_total = 0u128;
    let mut t_convert_total = 0u128;
    while cursor.advance().await.map_err(map_mongo_error)? {
        let t_fetch = std::time::Instant::now();
        let doc = cursor.deserialize_current().map_err(map_mongo_error)?;
        t_fetch_total += t_fetch.elapsed().as_micros();

        let t_convert = std::time::Instant::now();
        let row_idx = rows.len();
        for key in doc.keys() {
            if columns_set.insert(key.clone()) {
                columns.push(key.clone());
            }
        }
        let mut row = BTreeMap::new();
        for col in &columns {
            let val = doc.get(col);
            let (cell, mtype) = bson_to_cell_and_type(val);
            if let Some(mv) = mtype {
                mongo_types.insert((row_idx, col.clone()), mv);
            }
            row.insert(col.clone(), cell);
        }
        t_convert_total += t_convert.elapsed().as_micros();
        rows.push(row);
    }
    tracing::info!("[MongoDB] collect_cursor {} 行总耗时: {}ms (网络fetch={}ms, BSON转换={}ms, {}列)", rows.len(), t0.elapsed().as_millis(), t_fetch_total / 1000, t_convert_total / 1000, columns.len());
    Ok((columns, rows, mongo_types))
}

/// 一次匹配同时返回显示值和 MongoDB 特殊类型，避免对同一值做两次模式匹配
fn bson_to_cell_and_type(value: Option<&Bson>) -> (QueryCellValue, Option<MongoValue>) {
    match value {
        None => (QueryCellValue::Null, None),
        Some(b) => match b {
            Bson::Null => (QueryCellValue::Null, None),
            Bson::Int32(v) => (v.to_string().into(), None),
            Bson::Int64(v) => (v.to_string().into(), None),
            Bson::Double(v) => (v.to_string().into(), None),
            Bson::String(v) => (v.clone().into(), None),
            Bson::Boolean(v) => (v.to_string().into(), None),
            Bson::ObjectId(oid) => (oid.to_hex().into(), Some(MongoValue::ObjectId)),
            Bson::DateTime(dt) => {
                let sys_time: std::time::SystemTime = (*dt).into();
                let chrono_dt: chrono::DateTime<chrono::Utc> = sys_time.into();
                (chrono_dt.to_rfc3339().into(), Some(MongoValue::DateTime))
            }
            Bson::Timestamp(ts) => (
                format!("Timestamp({}, {})", ts.time, ts.increment).into(),
                Some(MongoValue::Timestamp),
            ),
            Bson::Binary(bin) => (hex::encode(&bin.bytes).into(), Some(MongoValue::Binary)),
            Bson::Array(arr) => {
                let items: Vec<String> = arr.iter().map(|v| bson_to_cell_text(v)).collect();
                (format!("[{}]", items.join(", ")).into(), None)
            }
            Bson::Document(doc) => (
                serde_json::to_string(doc)
                    .unwrap_or_else(|_| "{...}".to_string())
                    .into(),
                None,
            ),
            Bson::RegularExpression(re) => (
                format!("/{}/{}", re.pattern, re.options).into(),
                Some(MongoValue::RegularExpression),
            ),
            Bson::JavaScriptCode(code) => (code.clone().into(), None),
            Bson::Undefined => ("undefined".into(), None),
            Bson::Decimal128(d) => (d.to_string().into(), Some(MongoValue::Decimal128)),
            Bson::MaxKey => ("MaxKey".into(), None),
            Bson::MinKey => ("MinKey".into(), None),
            Bson::Symbol(s) => (s.clone().into(), None),
            other => (format!("{:?}", other).into(), None),
        },
    }
}

fn bson_to_mongo_value(value: Option<&Bson>) -> Option<MongoValue> {
    bson_to_cell_and_type(value).1
}

fn bson_to_cell(value: Option<&Bson>) -> QueryCellValue {
    bson_to_cell_and_type(value).0
}

fn bson_to_cell_text(b: &Bson) -> String {
    match b {
        Bson::String(s) => format!("\"{}\"", s),
        Bson::Null => "null".to_string(),
        other => bson_to_cell(Some(other)).display_text().to_string(),
    }
}

fn bson_type_name(b: &Bson) -> String {
    match b {
        Bson::Null => "null",
        Bson::Int32(_) => "Int32",
        Bson::Int64(_) => "Int64",
        Bson::Double(_) => "Double",
        Bson::String(_) => "String",
        Bson::Boolean(_) => "Boolean",
        Bson::ObjectId(_) => "ObjectId",
        Bson::DateTime(_) => "Date",
        Bson::Timestamp(_) => "Timestamp",
        Bson::Binary(_) => "Binary",
        Bson::Array(_) => "Array",
        Bson::Document(_) => "Object",
        Bson::Decimal128(_) => "Decimal128",
        Bson::RegularExpression(_) => "Regex",
        Bson::JavaScriptCode(_) => "JavaScript",
        Bson::MaxKey => "MaxKey",
        Bson::MinKey => "MinKey",
        Bson::Symbol(_) => "Symbol",
        Bson::Undefined => "Undefined",
        _ => "Unknown",
    }
    .to_string()
}

fn map_mongo_error(e: mongodb::error::Error) -> AppError {
    let err_str = e.to_string();
    if err_str.contains("not connected") || err_str.contains("connection pool")
        || err_str.contains("Server selection") || err_str.contains("network")
    {
        AppError::Connection(err_str)
    } else {
        AppError::Query(err_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_comments() {
        assert_eq!(strip_comments("// 注释\nshow dbs"), "\nshow dbs");
        assert_eq!(strip_comments("/* block */show dbs"), "show dbs");
        assert_eq!(strip_comments("db.a.find({//注释\n})"), "db.a.find({\n})");
        assert_eq!(strip_comments(r#"db.a.find({"x":"//not comment"})"#), r#"db.a.find({"x":"//not comment"})"#);
    }

    #[test]
    fn test_preprocess_number_decimal() {
        let input = r#"{balance: NumberDecimal("999.99")}"#;
        let result = preprocess_mongo_json(input);
        eprintln!("=== Input: {} ===", input);
        eprintln!("=== Input bytes: {:?} ===", input.as_bytes());
        eprintln!("=== Result: {} ===", result);
        eprintln!("=== Result bytes: {:?} ===", result.as_bytes());
        let v: serde_json::Value = serde_json::from_str(&result).expect(&format!("failed to parse: {result}"));
        assert_eq!(v["balance"]["$numberDecimal"], "999.99");
    }

    #[test]
    fn test_preprocess_new_date() {
        let input = r#"{createdAt: new Date()}"#;
        let result = preprocess_mongo_json(input);
        let v: serde_json::Value = serde_json::from_str(&result).expect(&format!("failed to parse: {result}"));
        assert!(v["createdAt"]["$date"].is_string());
    }

    #[test]
    fn test_preprocess_full_document() {
        let input = r#"{name: "测试用户", age: 25, balance: NumberDecimal("999.99"), isActive: true, createdAt: new Date(), hobbies: ["coding", "reading"], address: {city: "北京", district: "海淀"}}"#;
        let result = preprocess_mongo_json(input);
        eprintln!("=== Result: {} ===", result);
        let v: serde_json::Value = serde_json::from_str(&result).expect("failed to parse JSON");
        assert_eq!(v["name"], "测试用户", "name mismatch");
        assert_eq!(v["age"], 25, "age mismatch");
        assert_eq!(v["balance"]["$numberDecimal"], "999.99");
        assert_eq!(v["isActive"], true, "isActive mismatch");
        assert!(v["createdAt"]["$date"].is_string(), "createdAt should have $date");
        assert_eq!(v["hobbies"][0], "coding", "hobbies[0] mismatch");
        assert_eq!(v["hobbies"][1], "reading", "hobbies[1] mismatch");
        assert_eq!(v["address"]["city"], "北京", "address.city mismatch");
        assert_eq!(v["address"]["district"], "海淀", "address.district mismatch");
    }

    #[test]
    fn test_collect_document_fields_flattens_nested_paths() {
        let doc = doc! {
            "profile": {
                "name": "alice",
                "stats": { "score": 42 }
            },
            "tags": [
                { "label": "vip" }
            ]
        };
        let mut fields = Vec::new();
        let mut seen = HashSet::new();
        collect_document_fields(&doc, None, &mut fields, &mut seen);
        let names: HashSet<_> = fields.into_iter().map(|f| f.name).collect();
        assert!(names.contains("profile"));
        assert!(names.contains("profile.name"));
        assert!(names.contains("profile.stats"));
        assert!(names.contains("profile.stats.score"));
        assert!(names.contains("tags"));
        assert!(names.contains("tags.label"));
    }
}
