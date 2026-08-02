//! SQL 括号折叠的纯逻辑：候选检测、偏移映射、显示文本构造。
//! 不依赖 egui，便于单元测试。

/// 一个可折叠的跨行括号块。
/// `hide_start`/`hide_end` 为 **byte index**（供 `&sql[a..b]` 切片，多字节安全）；
/// `open_line`/`open_col` 为 char 语义（行号 / 行内列偏移）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoldCandidate {
    /// 开括号所在行（1-based）
    pub open_line: usize,
    /// 开括号列（该行内 char 偏移）
    pub open_col: usize,
    /// 折叠隐藏区起始 byte index = 开括号行换行之后
    pub hide_start: usize,
    /// 折叠隐藏区结束 byte index = 匹配的闭括号 ')' 之前
    pub hide_end: usize,
}

/// 找到所有跨多行（区域含至少一个 '\n'）的 `(...)` 块。
/// 沿用 find_matching_paren 的 quote/escape 跳过规则。
pub fn find_fold_candidates(sql: &str) -> Vec<FoldCandidate> {
    let mut stack: Vec<(usize, usize, usize)> = Vec::new(); // (open_paren_char_idx, open_col, open_paren_byte_idx)
    let mut out = Vec::new();
    let mut quote: Option<char> = None;
    let mut escape = false;
    let mut col = 0usize; // 当前行内已扫描的 char 数（开括号列以此计）
    // byte_pos 记录当前 char 的起始 byte 位置，hide_start/hide_end 以 byte index 存储（供 &str 切片）
    for (i, (byte_pos, ch)) in sql.char_indices().enumerate() {
        if let Some(q) = quote {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == q {
                quote = None;
            }
        } else {
            match ch {
                '\'' | '"' | '`' => quote = Some(ch),
                '(' => stack.push((i, col, byte_pos)),
                ')' => {
                    if let Some((open_idx, open_col, open_byte)) = stack.pop() {
                        // 开括号与闭括号之间是否跨行：检测开括号之后到闭括号之前是否有 '\n'
                        let crosses_lines = sql[open_byte..byte_pos].chars().any(|c| c == '\n');
                        if crosses_lines {
                            // 隐藏区起始 = 开括号后第一个换行的下一个字符（byte 位置）
                            let nl_byte = sql[open_byte..byte_pos]
                                .find('\n')
                                .map(|p| open_byte + p);
                            if let Some(nl_byte) = nl_byte {
                                out.push(FoldCandidate {
                                    open_line: line_of_char(sql, open_idx),
                                    open_col,
                                    hide_start: nl_byte + '\n'.len_utf8(),
                                    hide_end: byte_pos,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // 换行统一在此处理（字符串内部与外部一致），复位列计数
        if ch == '\n' {
            col = 0;
        } else {
            col += 1;
        }
    }
    out
}

/// 折叠区域内容哈希：用于编辑导致行号漂移后判断折叠是否保留。
/// `c.hide_start/hide_end` 为 byte index，直接对字节流做 FNV-1a（与 char 无关）。
pub fn fold_content_hash(sql: &str, c: &FoldCandidate) -> u64 {
    let mut h: u64 = 1469598103934665603;
    for b in sql[c.hide_start..c.hide_end].as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

/// char 索引 → 1-based 行号。换行符计入其后的行：char_index 处为换行符时归属下一行。
pub fn line_of_char(sql: &str, char_index: usize) -> usize {
    let mut line = 1usize;
    for (i, ch) in sql.char_indices() {
        if i > char_index {
            break;
        }
        if ch == '\n' {
            line += 1;
        }
    }
    line
}

/// 1-based 行号 → 该行首个字符的 char 索引。
/// 换行归属约定与 line_of_char 一致：字符串中最后一个换行符归属其后的行（作为该行起始）；
/// 其余换行符终止当前行，下一行从其后的字符开始。
pub fn char_line_start(sql: &str, line: usize) -> usize {
    let last_nl = sql.rfind('\n');
    let mut cur = 1usize;
    let mut start = 0usize;
    for (i, ch) in sql.char_indices() {
        if cur == line {
            break;
        }
        if ch == '\n' {
            cur += 1;
            start = if last_nl == Some(i) { i } else { i + 1 };
        }
    }
    start
}

/// byte index → char index（要求 byte 落在 char 边界上，内部用）。
fn byte_to_char(sql: &str, byte_idx: usize) -> usize {
    sql[..byte_idx].chars().count()
}

/// 已折叠区域：按"起始行 + 内容哈希"识别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoldRegion {
    pub open_line: usize,
    pub content_hash: u64,
}

/// 折叠区域的显示段。
#[derive(Debug, Clone, Copy)]
pub struct FoldSegment {
    pub sql_start: usize,
    pub sql_end: usize,
    pub display_start: usize,
    pub display_end: usize,
}

/// 折叠后的显示文本及其偏移映射。
#[derive(Debug, Clone)]
pub struct FoldDisplay {
    pub text: String,
    pub segments: Vec<FoldSegment>,
}

impl FoldDisplay {
    /// sql char 索引 → display char 索引。落在折叠区内钳制到占位符末尾。
    pub fn sql_to_display(&self, sql_idx: usize) -> usize {
        let mut delta: isize = 0;
        for seg in &self.segments {
            if sql_idx < seg.sql_start {
                break;
            }
            if sql_idx < seg.sql_end {
                // 钳制到占位符末尾
                return (seg.display_end as isize) as usize;
            }
            delta = (seg.display_end as isize) - (seg.sql_end as isize);
        }
        ((sql_idx as isize) + delta) as usize
    }

    /// display char 索引 → sql char 索引。落在占位符上 → 折叠区末尾。
    pub fn display_to_sql(&self, display_idx: usize) -> usize {
        let mut delta: isize = 0;
        for seg in &self.segments {
            if display_idx < seg.display_start {
                break;
            }
            if display_idx < seg.display_end {
                return seg.sql_end;
            }
            delta = (seg.sql_end as isize) - (seg.display_end as isize);
        }
        ((display_idx as isize) + delta) as usize
    }

    /// sql 索引是否落在某折叠区内
    pub fn region_contains_sql_idx(&self, sql_idx: usize) -> bool {
        self.segments
            .iter()
            .any(|seg| sql_idx >= seg.sql_start && sql_idx < seg.sql_end)
    }
}

/// 把显示文本上的编辑 diff 换算回 sql 空间。
/// 返回 (del_start, del_end, inserted)，均为 sql char 索引。
/// 若编辑触及折叠区（调用方应先展开），返回 (0, 0, "") 表示无效、需重试。
pub fn map_display_edit_to_sql(d: &FoldDisplay, sql: &str, del_start: usize, del_end: usize, inserted: &str) -> (usize, usize, String) {
    let sql_del_start = d.display_to_sql(del_start);
    let sql_del_end = d.display_to_sql(del_end);
    if d.region_contains_sql_idx(sql_del_start)
        || (sql_del_end > sql_del_start && d.region_contains_sql_idx(sql_del_end.saturating_sub(1)))
    {
        return (0, 0, String::new());
    }
    let _ = sql;
    (sql_del_start, sql_del_end, inserted.to_string())
}

/// 构造显示文本；无折叠返回 None。
/// candidates 按 open_line 升序；只折叠 candidates 中与 folds 匹配且不被更外层折叠包含的。
pub fn build_display(sql: &str, folds: &[FoldRegion], candidates: &[FoldCandidate]) -> Option<FoldDisplay> {
    if folds.is_empty() {
        return None;
    }
    // 匹配候选：open_line + hash 一致
    let mut matched: Vec<&FoldCandidate> = candidates
        .iter()
        .filter(|c| folds.iter().any(|f| f.open_line == c.open_line && f.content_hash == fold_content_hash(sql, c)))
        .collect();
    if matched.is_empty() {
        return None;
    }
    // 剔除被更外层折叠包含的（嵌套只显示最外层）
    matched.sort_by_key(|c| c.hide_start);
    let mut filtered: Vec<&FoldCandidate> = Vec::new();
    for c in matched {
        if filtered.iter().any(|f: &&FoldCandidate| c.hide_start >= f.hide_start && c.hide_end <= f.hide_end) {
            continue;
        }
        filtered.push(c);
    }

    let mut text = String::with_capacity(sql.len());
    let mut segments = Vec::new();
    let mut cursor = 0usize; // sql 空间 byte 游标，推进到 c.hide_end
    let mut display_cursor = 0usize; // display 空间 char 偏移（egui CCursor 契约）
    for c in filtered {
        if c.hide_start > cursor {
            let frag = &sql[cursor..c.hide_start];
            text.push_str(frag);
            // display_cursor 累计 push 的 char 数，不能用 byte 差（hide_start 为 byte index）
            display_cursor += frag.chars().count();
        }
        let seg = FoldSegment {
            // FoldDisplay.segments 保持 char index 契约（egui CCursor 使用），由 byte 换算
            sql_start: byte_to_char(sql, c.hide_start),
            sql_end: byte_to_char(sql, c.hide_end),
            display_start: display_cursor,
            display_end: display_cursor + 2,
        };
        text.push('…');
        text.push('\n');
        display_cursor += 2;
        segments.push(seg);
        cursor = c.hide_end;
    }
    if cursor < sql.len() {
        text.push_str(&sql[cursor..]);
    }
    Some(FoldDisplay { text, segments })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_multiline_paren_block() {
        let sql = "SELECT * FROM users\nWHERE id IN (\n  SELECT id FROM orders\n)\nAND x = 1";
        let cands = find_fold_candidates(sql);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].open_line, 2);
    }

    #[test]
    fn ignores_singleline_parens() {
        let sql = "SELECT COALESCE(a, b) FROM t";
        assert!(find_fold_candidates(sql).is_empty());
    }

    #[test]
    fn nested_blocks_both_found() {
        let sql = "f(\n  g(\n    1\n  )\n)";
        let cands = find_fold_candidates(sql);
        assert_eq!(cands.len(), 2);
    }

    #[test]
    fn ignores_parens_in_quotes_and_backticks() {
        let sql = "SELECT 'a(b)\nc)', `col(x\n)` FROM t";
        assert!(find_fold_candidates(sql).is_empty());
    }

    #[test]
    fn hides_after_open_line_newline() {
        let sql = "foo(\n  bar\n)\nbaz";
        let c = &find_fold_candidates(sql)[0];
        // hide_start 是 "foo(\n" 之后，hide_end 是闭括号 ')' 之前
        assert_eq!(&sql[c.hide_start..c.hide_end], "  bar\n");
    }

    #[test]
    fn line_of_char_and_start_roundtrip() {
        let sql = "ab\ncd\ne";
        assert_eq!(line_of_char(sql, 0), 1);
        assert_eq!(line_of_char(sql, 3), 2);
        assert_eq!(line_of_char(sql, 5), 3);
        assert_eq!(char_line_start(sql, 2), 3);
        assert_eq!(char_line_start(sql, 3), 5);
    }

    #[test]
    fn hash_differs_for_different_content() {
        let sql_a = "f(\n  x\n)";
        let sql_b = "f(\n  y\n)";
        let a = find_fold_candidates(sql_a)[0];
        let b = find_fold_candidates(sql_b)[0];
        assert_ne!(fold_content_hash(sql_a, &a), fold_content_hash(sql_b, &b));
    }

    #[test]
    fn build_display_single_fold() {
        let sql = "f(\n  bar\n)\nbaz";
        let cands = find_fold_candidates(sql);
        let folds = vec![FoldRegion { open_line: cands[0].open_line, content_hash: fold_content_hash(sql, &cands[0]) }];
        let d = build_display(sql, &folds, &cands).unwrap();
        assert_eq!(d.text, "f(\n…\n)\nbaz");
    }

    #[test]
    fn build_display_no_folds_returns_none() {
        let sql = "SELECT 1";
        let cands = find_fold_candidates(sql);
        assert!(build_display(sql, &[], &cands).is_none());
    }

    #[test]
    fn nested_only_outer_folds() {
        // 外层折叠时内层候选被包含，display 只替换外层区域
        let sql = "f(\n  g(\n    1\n  )\n)";
        let cands = find_fold_candidates(sql);
        // find_fold_candidates 按闭括号相遇顺序返回：cands[0] 是内层 g(...)
        let outer = cands.iter().find(|c| c.open_line == 1).unwrap();
        let folds = vec![FoldRegion { open_line: outer.open_line, content_hash: fold_content_hash(sql, outer) }];
        let d = build_display(sql, &folds, &cands).unwrap();
        assert_eq!(d.text, "f(\n…\n)");
    }

    #[test]
    fn offset_mapping_roundtrip() {
        let sql = "aa\nf(\n  bar\n)\nzz";
        let cands = find_fold_candidates(sql);
        let folds = vec![FoldRegion { open_line: cands[0].open_line, content_hash: fold_content_hash(sql, &cands[0]) }];
        let d = build_display(sql, &folds, &cands).unwrap();
        // sql 索引 → display → 回到 sql（除折叠区内钳制）
        let sql_idx = sql.len(); // 末尾
        let disp = d.sql_to_display(sql_idx);
        // 整体文本与"替换折叠区为占位符"等价（char 级比较）
        assert_eq!(d.text, sql.replace(&sql[cands[0].hide_start..cands[0].hide_end], "…\n"));
        // display 末尾索引回 sql 末尾
        let back = d.display_to_sql(disp);
        assert_eq!(back, sql_idx);
    }

    #[test]
    fn cursor_in_fold_region_detected() {
        let sql = "f(\n  bar\n)\nbaz";
        let cands = find_fold_candidates(sql);
        let folds = vec![FoldRegion { open_line: cands[0].open_line, content_hash: fold_content_hash(sql, &cands[0]) }];
        let d = build_display(sql, &folds, &cands).unwrap();
        // segments 为 char index 空间，region_contains_sql_idx 亦为 char 空间
        let seg = d.segments[0];
        assert!(d.region_contains_sql_idx(seg.sql_start + 1));
        assert!(!d.region_contains_sql_idx(seg.sql_start.saturating_sub(1)));
    }

    #[test]
    fn edit_before_fold_maps_to_sql() {
        let sql = "aa\nf(\n  bar\n)\nzz";
        let cands = find_fold_candidates(sql);
        let folds = vec![FoldRegion { open_line: cands[0].open_line, content_hash: fold_content_hash(sql, &cands[0]) }];
        let d = build_display(sql, &folds, &cands).unwrap();
        // 在 display "aa\n" 后插入 "XX"
        let (ds, de, ins) = map_display_edit_to_sql(&d, sql, 3, 3, "XX");
        assert_eq!((ds, de), (3, 3));
        assert_eq!(ins, "XX");
    }

    #[test]
    fn edit_after_fold_maps_correctly() {
        let sql = "f(\n  bar\n)\nbaz";
        let cands = find_fold_candidates(sql);
        let folds = vec![FoldRegion { open_line: cands[0].open_line, content_hash: fold_content_hash(sql, &cands[0]) }];
        let d = build_display(sql, &folds, &cands).unwrap();
        // "baz" 起始处插入（display char 索引 = 占位符结束 + 闭括号 + 换行 = display_end + 2）。
        // 注意不能以 d.text.len() 为索引：它返回字节长度，"…" 占 3 字节，与 char 索引不一致。
        let seg = &d.segments[0];
        let disp_baz = seg.display_end + 2;
        let (ds, de, ins) = map_display_edit_to_sql(&d, sql, disp_baz, disp_baz, "Q");
        // sql 中 "baz" 起始 = 折叠区结束 + 闭括号 + 换行 = sql_end + 2
        let expect = seg.sql_end + 2;
        assert_eq!((ds, de), (expect, expect));
        assert_eq!(ins, "Q");
    }

    #[test]
    fn edit_touching_fold_returns_invalid_signal() {
        let sql = "f(\n  bar\n)\nbaz";
        let cands = find_fold_candidates(sql);
        let folds = vec![FoldRegion { open_line: cands[0].open_line, content_hash: fold_content_hash(sql, &cands[0]) }];
        let d = build_display(sql, &folds, &cands).unwrap();
        // 删除 display 2..4（"…" 占位符及其前导换行）：起点落在折叠区之前（sql 2）、终点落在区内（sql 9，end-1=8 ∈ [hide_start, hide_end)）
        let (ds, de, ins) = map_display_edit_to_sql(&d, sql, 2, 4, "");
        assert_eq!((ds, de), (0, 0));
        assert_eq!(ins, "");
    }

    #[test]
    fn nested_expand_reveals_inner() {
        // 外层展开后，内层折叠仍有效（folded 保留内层）
        let sql = "f(\n  g(\n    1\n  )\n)";
        let cands = find_fold_candidates(sql);
        // 只折叠内层
        let inner = cands.iter().find(|c| c.open_line == 2).unwrap();
        let folds = vec![FoldRegion { open_line: inner.open_line, content_hash: fold_content_hash(sql, inner) }];
        let d = build_display(sql, &folds, &cands).unwrap();
        // 注：hide_end 为闭括号 ')' 索引，隐藏区含闭括号前缩进（与 build_display_single_fold 语义一致）
        assert_eq!(d.text, "f(\n  g(\n…\n)\n)");
    }

    #[test]
    fn stale_fold_discarded_after_edit() {
        // 编辑改变内容 → content_hash 变化 → refresh_folds 丢弃
        let sql = "f(\n  bar\n)\nbaz";
        let cands = find_fold_candidates(sql);
        let mut folds = vec![FoldRegion { open_line: cands[0].open_line, content_hash: fold_content_hash(sql, &cands[0]) }];
        // 模拟编辑：内容变了
        let new_sql = "f(\n  CHANGED\n)\nbaz";
        let new_cands = find_fold_candidates(new_sql);
        folds.retain(|f| {
            new_cands.iter().any(|c| c.open_line == f.open_line && fold_content_hash(new_sql, c) == f.content_hash)
        });
        assert!(folds.is_empty());
    }

    // ── C-1 回归：多字节字符（中文/©）下 byte 切片不 panic ──

    #[test]
    fn multibyte_content_hash_does_not_panic() {
        // 中文注释等非 ASCII 字符 + 折叠边界：hide_start/hide_end 为 byte index，byte 切片安全
        let sql = "f(\n-- 中文注释\n'val'\n)";
        let cands = find_fold_candidates(sql);
        assert_eq!(cands.len(), 1);
        let _ = fold_content_hash(sql, &cands[0]);
    }

    #[test]
    fn multibyte_2byte_char_does_not_panic() {
        // 评审触发示例：© 是 2 字节字符
        let sql = "f(\n©\n)";
        let cands = find_fold_candidates(sql);
        assert_eq!(cands.len(), 1);
        let _ = fold_content_hash(sql, &cands[0]);
        let folds = vec![FoldRegion { open_line: cands[0].open_line, content_hash: fold_content_hash(sql, &cands[0]) }];
        let d = build_display(sql, &folds, &cands).unwrap();
        // 折叠区 = "©\n"，display 文本为前缀 + 占位符 + 闭括号
        assert_eq!(d.text, "f(\n…\n)");
        // segments 保持 char index 契约：隐藏区 char 区间 [3,5)
        assert_eq!(d.segments[0].sql_start, 3);
        assert_eq!(d.segments[0].sql_end, 5);
    }

    #[test]
    fn build_display_multibyte_does_not_panic() {
        let sql = "f(\n-- 中文注释\n'val'\n)";
        let cands = find_fold_candidates(sql);
        let folds = vec![FoldRegion { open_line: cands[0].open_line, content_hash: fold_content_hash(sql, &cands[0]) }];
        let d = build_display(sql, &folds, &cands).unwrap();
        // 折叠区整行含中文注释，display 文本为前缀 + 占位符 + 闭括号
        assert_eq!(d.text, "f(\n…\n)");
        // segments 保持 char index 契约：隐藏区 char 区间 [3,17)
        assert_eq!(d.segments[0].sql_start, 3);
        assert_eq!(d.segments[0].sql_end, 17);
    }

    #[test]
    fn multibyte_offset_mapping_roundtrip() {
        // 折叠区前有中文 → byte/char 混合索引下偏移映射往返仍闭合
        let sql = "SELECT '中' FROM t\nf(\n-- 中文注释\n'val'\n)\nzz";
        let cands = find_fold_candidates(sql);
        assert_eq!(cands.len(), 1);
        let folds = vec![FoldRegion { open_line: cands[0].open_line, content_hash: fold_content_hash(sql, &cands[0]) }];
        let d = build_display(sql, &folds, &cands).unwrap();
        // 文本末尾往返
        let char_len = sql.chars().count();
        let disp = d.sql_to_display(char_len);
        assert_eq!(d.display_to_sql(disp), char_len);
        // 折叠区内部钳制到占位符末尾
        let inside = d.segments[0].sql_start + 1;
        assert!(d.region_contains_sql_idx(inside));
        assert_eq!(d.sql_to_display(inside), d.segments[0].display_end);
    }
}
