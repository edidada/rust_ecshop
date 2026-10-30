//! Translation of repository-owned SQLite statement forms at the database boundary.
//! Bound values never enter this function. This is not a general SQL transpiler.
pub fn mysql(sql: &str) -> String {
    let mut result = sql.trim().to_string();
    if result.starts_with("INSERT OR IGNORE ") {
        result = result.replacen("INSERT OR IGNORE ", "INSERT IGNORE ", 1);
    }
    if result.starts_with("INSERT OR REPLACE ") {
        result = result.replacen("INSERT OR REPLACE ", "REPLACE ", 1);
    }
    if result.starts_with("UPDATE OR IGNORE ") {
        result = result.replacen("UPDATE OR IGNORE ", "UPDATE IGNORE ", 1);
    }
    if let Some(start) = result.find("ON CONFLICT(") {
        if let Some(offset) = result[start..].find("DO UPDATE SET") {
            result.replace_range(
                start..start + offset + "DO UPDATE SET".len(),
                "ON DUPLICATE KEY UPDATE",
            );
            while let Some(pos) = result.find("excluded.") {
                let begin = pos + 9;
                let length = result[begin..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .count();
                if length == 0 {
                    break;
                }
                let column = result[begin..begin + length].to_string();
                result.replace_range(pos..begin + length, &format!("VALUES({column})"));
            }
            // Qualify target values to avoid ambiguity with INSERT ... SELECT.
            result = result.replace("MIN(goods_number +", "LEAST(goods_number +");
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upsert_keeps_bind_order_and_excluded_values() {
        let input =
            "INSERT INTO t(id,n) VALUES(?,?) ON CONFLICT(id) DO UPDATE SET n=excluded.n, id=?";
        assert_eq!(
            mysql(input),
            "INSERT INTO t(id,n) VALUES(?,?) ON DUPLICATE KEY UPDATE n=VALUES(n), id=?"
        );
    }
    #[test]
    fn ordinary_query_and_literals_are_unchanged() {
        let sql = "SELECT 'INSERT OR IGNORE' AS label FROM ecs_goods WHERE goods_id=?";
        assert_eq!(mysql(sql), sql);
    }
}
