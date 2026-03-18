use tabled::{Table, Tabled};

use crate::registry::MenuEntry;

#[derive(Tabled)]
struct Row {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    entry_type: String,
    #[tabled(rename = "Location")]
    location: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Command / CLSID")]
    command: String,
}

pub fn print_table(entries: &[MenuEntry]) {
    let rows: Vec<Row> = entries
        .iter()
        .map(|e| Row {
            name: e.name.clone(),
            entry_type: e.entry_type.to_string(),
            location: e.location.to_string(),
            status: e.status.to_string(),
            command: truncate(e.command.as_deref().unwrap_or(""), 60),
        })
        .collect();

    println!("{}", Table::new(rows));
    println!("\n{} entries total", entries.len());
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}
