use core_domain::SavedQueryEntry;

/// 树节点：连接 → 库 → 查询
#[derive(Debug, Clone, PartialEq)]
pub enum QueryTreeNode {
    /// 连接节点：key 用于折叠状态持久化
    Connection {
        key: String,
        display_name: String,
        children: Vec<QueryTreeNode>,
    },
    /// 库节点（或无库查询的平铺分组）
    Database {
        key: String,
        name: String,
        children: Vec<QueryTreeNode>,
    },
    /// 查询叶子
    Query {
        entry_id: String,
        title: String,
        connection_id: String,
        database: Option<String>,
        sql_text: String,
    },
}

pub fn conn_node_key(id: &str) -> String {
    format!("conn:{id}")
}

pub fn db_node_key(conn_id: &str, db: &str) -> String {
    format!("db:{conn_id}/{db}")
}

fn conn_display_name(entry: &SavedQueryEntry, live: &dyn Fn(&str) -> Option<String>) -> String {
    if let Some(live_name) = live(&entry.connection_id) {
        return live_name;
    }
    if let Some(snap) = &entry.connection_name {
        if !snap.is_empty() {
            return snap.clone();
        }
    }
    entry.connection_id.clone()
}

pub fn build_tree(
    entries: &[SavedQueryEntry],
    live: &dyn Fn(&str) -> Option<String>,
) -> Vec<QueryTreeNode> {
    // 按连接 id 分组（保持插入顺序，后续按名称排序）
    let mut conn_order: Vec<String> = Vec::new();
    let mut conn_map: std::collections::HashMap<String, Vec<&SavedQueryEntry>> =
        std::collections::HashMap::new();
    for e in entries {
        if !conn_map.contains_key(&e.connection_id) {
            conn_order.push(e.connection_id.clone());
        }
        conn_map.entry(e.connection_id.clone()).or_default().push(e);
    }

    let mut nodes: Vec<QueryTreeNode> = Vec::new();
    for conn_id in conn_order {
        let group = conn_map.get(&conn_id).unwrap();
        // 组内按 sort_order 排序
        let mut sorted: Vec<&SavedQueryEntry> = group.clone();
        sorted.sort_by_key(|e| e.sort_order);
        let display_name = sorted
            .first()
            .map(|e| conn_display_name(e, live))
            .unwrap_or_else(|| conn_id.clone());
        // 库分组
        let mut db_order: Vec<String> = Vec::new();
        let mut db_map: std::collections::HashMap<String, Vec<&SavedQueryEntry>> =
            std::collections::HashMap::new();
        let mut flat: Vec<&SavedQueryEntry> = Vec::new(); // 无库查询
        for e in &sorted {
            if let Some(ref db) = e.database {
                if !db_map.contains_key(db.as_str()) {
                    db_order.push(db.clone());
                }
                db_map.entry(db.clone()).or_default().push(e);
            } else {
                flat.push(e);
            }
        }
        let mut children: Vec<QueryTreeNode> = Vec::new();
        // 库节点按名称排序
        db_order.sort();
        for db in db_order {
            let db_children: Vec<QueryTreeNode> = db_map[&db]
                .iter()
                .map(|e| QueryTreeNode::Query {
                    entry_id: e.id.clone(),
                    title: e.title.clone(),
                    connection_id: e.connection_id.clone(),
                    database: e.database.clone(),
                    sql_text: e.sql_text.clone(),
                })
                .collect();
            // 库内查询已按 sort_order 顺序构建（sorted 保序），不重排
            children.push(QueryTreeNode::Database {
                key: db_node_key(&conn_id, &db),
                name: db.clone(),
                children: db_children,
            });
        }
        for e in flat {
            children.push(QueryTreeNode::Query {
                entry_id: e.id.clone(),
                title: e.title.clone(),
                connection_id: e.connection_id.clone(),
                database: None,
                sql_text: e.sql_text.clone(),
            });
        }
        nodes.push(QueryTreeNode::Connection {
            key: conn_node_key(&conn_id),
            display_name,
            children,
        });
    }
    // 连接节点按显示名称排序
    nodes.sort_by(|a, b| match (a, b) {
        (QueryTreeNode::Connection { display_name: x, .. }, QueryTreeNode::Connection { display_name: y, .. }) => x.cmp(y),
        _ => std::cmp::Ordering::Equal,
    });
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn entry(id: &str, conn_id: &str, db: Option<&str>, title: &str, sort: i32) -> SavedQueryEntry {
        SavedQueryEntry {
            id: id.into(),
            connection_id: conn_id.into(),
            database: db.map(String::from),
            title: title.into(),
            sql_text: "SELECT 1".into(),
            saved_at: Utc::now(),
            sort_order: sort,
            connection_name: Some(format!("conn-{}", conn_id)),
        }
    }

    #[test]
    fn builds_conn_db_query_hierarchy() {
        let entries = vec![
            entry("q1", "c1", Some("db1"), "查询A", 0),
            entry("q2", "c1", Some("db1"), "查询B", 1),
            entry("q3", "c1", Some("db2"), "查询C", 0),
            entry("q4", "c2", None, "无库查询", 0),
        ];
        let live = |id: &str| -> Option<String> { Some(format!("实时名-{}", id)) };
        let tree = build_tree(&entries, &live);
        assert_eq!(tree.len(), 2);
        // 连接按名称排序（c1 < c2）
        match &tree[0] {
            QueryTreeNode::Connection { children, .. } => {
                assert_eq!(children.len(), 2); // db1, db2
                assert!(matches!(&children[0], QueryTreeNode::Database { name, .. } if name == "db1"));
                assert!(matches!(&children[1], QueryTreeNode::Database { name, .. } if name == "db2"));
                match &children[0] {
                    QueryTreeNode::Database { children, .. } => assert_eq!(children.len(), 2),
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
        // c2 连接下无库查询平铺
        match &tree[1] {
            QueryTreeNode::Connection { children, .. } => {
                assert!(matches!(&children[0], QueryTreeNode::Query { title, .. } if title == "无库查询"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn live_name_prefers_live_over_snapshot() {
        let entries = vec![entry("q1", "c1", Some("db1"), "查询A", 0)];
        let live = |id: &str| -> Option<String> { Some(format!("LIVE-{}", id)) };
        let tree = build_tree(&entries, &live);
        match &tree[0] {
            QueryTreeNode::Connection { display_name, .. } => assert_eq!(display_name, "LIVE-c1"),
            _ => panic!(),
        }
    }

    #[test]
    fn snapshot_fallback_when_connection_deleted() {
        let entries = vec![entry("q1", "c1", Some("db1"), "查询A", 0)];
        let live = |_id: &str| -> Option<String> { None }; // 连接已删除
        let tree = build_tree(&entries, &live);
        match &tree[0] {
            QueryTreeNode::Connection { display_name, .. } => assert_eq!(display_name, "conn-c1"), // 快照
            _ => panic!(),
        }
    }
}
