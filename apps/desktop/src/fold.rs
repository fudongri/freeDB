//! SQL 括号折叠的纯逻辑：候选检测、偏移映射、显示文本构造。
//! 不依赖 egui，便于单元测试。

/// 一个可折叠的跨行括号块。所有索引均为 char index（与 egui CCursor 一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoldCandidate {
    /// 开括号所在行（1-based）
    pub open_line: usize,
    /// 开括号列（该行内 char 偏移）
    pub open_col: usize,
    /// 折叠隐藏区起始 char 索引 = 开括号行换行之后
    pub hide_start: usize,
    /// 折叠隐藏区结束 char 索引 = 匹配的闭括号 ')' 之前
    pub hide_end: usize,
}

/// 找到所有跨多行（区域含至少一个 '\n'）的 `(...)` 块。
/// 沿用 find_matching_paren 的 quote/escape 跳过规则。
pub fn find_fold_candidates(sql: &str) -> Vec<FoldCandidate> {
    let chars: Vec<char> = sql.chars().collect();
    let mut stack: Vec<(usize, usize)> = Vec::new(); // (open_paren_char_idx, open_col)
    let mut out = Vec::new();
    let mut quote: Option<char> = None;
    let mut escape = false;
    let mut col = 0usize; // 当前行内已扫描的 char 数（开括号列以此计）
    for (i, &ch) in chars.iter().enumerate() {
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
                '(' => stack.push((i, col)),
                ')' => {
                    if let Some((open_idx, open_col)) = stack.pop() {
                        // 开括号与闭括号之间是否跨行：检测开括号之后到闭括号之前是否有 '\n'
                        let crosses_lines = chars[open_idx + 1..i].iter().any(|&c| c == '\n');
                        if crosses_lines {
                            // 隐藏区起始 = 开括号后第一个换行的下一个字符
                            let nl_idx = chars[open_idx..i]
                                .iter()
                                .position(|&c| c == '\n')
                                .map(|p| open_idx + p);
                            if let Some(nl_idx) = nl_idx {
                                out.push(FoldCandidate {
                                    open_line: line_of_char(sql, open_idx),
                                    open_col,
                                    hide_start: nl_idx + 1,
                                    hide_end: i,
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
    let mut cursor = 0usize;
    let mut display_cursor = 0usize;
    for c in filtered {
        if c.hide_start > cursor {
            text.push_str(&sql[cursor..c.hide_start]);
            display_cursor += c.hide_start - cursor;
        }
        let seg = FoldSegment {
            sql_start: c.hide_start,
            sql_end: c.hide_end,
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
        let c = &cands[0];
        assert!(d.region_contains_sql_idx(c.hide_start + 1));
        assert!(!d.region_contains_sql_idx(c.hide_start - 1));
    }
}
