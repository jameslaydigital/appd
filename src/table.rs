//! Minimal column formatter. No dependency, no colour, no wrapping.

pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(headers: &[&str]) -> Self {
        Self {
            headers: headers.iter().map(|h| h.to_string()).collect(),
            rows: Vec::new(),
        }
    }

    pub fn push(&mut self, row: Vec<String>) {
        debug_assert_eq!(row.len(), self.headers.len());
        self.rows.push(row);
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn render(&self) -> String {
        let columns = self.headers.len();
        let mut widths: Vec<usize> = self.headers.iter().map(|h| h.chars().count()).collect();
        for row in &self.rows {
            for (index, cell) in row.iter().enumerate() {
                widths[index] = widths[index].max(cell.chars().count());
            }
        }

        let mut out = String::new();
        for (index, header) in self.headers.iter().enumerate() {
            push_cell(&mut out, header, widths[index], index + 1 == columns);
        }
        out.push('\n');

        for row in &self.rows {
            for (index, cell) in row.iter().enumerate() {
                push_cell(&mut out, cell, widths[index], index + 1 == columns);
            }
            out.push('\n');
        }
        out
    }
}

fn push_cell(out: &mut String, cell: &str, width: usize, last: bool) {
    out.push_str(cell);
    if last {
        return;
    }
    let padding = width.saturating_sub(cell.chars().count()) + 2;
    out.push_str(&" ".repeat(padding));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligns_columns_and_trims_trailing_space() {
        let mut table = Table::new(&["APP", "STATE"]);
        table.push(vec!["goal-tracker".into(), "running".into()]);
        table.push(vec!["api".into(), "failed".into()]);

        let rendered = table.render();
        let lines: Vec<&str> = rendered.lines().collect();

        assert_eq!(lines[0], "APP           STATE");
        assert_eq!(lines[1], "goal-tracker  running");
        assert_eq!(lines[2], "api           failed");
    }
}
