use std::sync::Arc;

pub(in crate::ui_runtime::presentation) const CONTENT_FILE_GAP: f32 = 100.0;
const TEXT_ROW_GAP: f32 = 10.0;

#[derive(Clone, Debug)]
pub(super) struct Row {
    pub(super) text: Arc<str>,
    pub(super) centered: bool,
    pub(super) gap: f32,
    pub(super) blank_line: bool,
}

#[derive(Clone, Debug)]
pub(super) struct Content {
    pub(super) rows: Arc<[Row]>,
    pub(super) missing: bool,
}

impl Content {
    pub(super) fn read(read: impl FnMut(&str) -> Option<Vec<u8>>) -> Self {
        let mut rows = Vec::new();
        let [poem, credits, quote] = assets::UI_CREDITS_FILES.map(read);
        let missing = poem.is_none() || credits.is_none() || quote.is_none();
        if let Some(text) = poem
            .as_deref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
        {
            file_separator(&mut rows);
            text_rows(&mut rows, text);
        }
        if let Some(value) = credits
            .as_deref()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(bytes).ok())
        {
            file_separator(&mut rows);
            json_rows(&mut rows, &value);
        }
        if let Some(text) = quote
            .as_deref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
        {
            file_separator(&mut rows);
            text_rows(&mut rows, text);
        }
        Self {
            rows: rows.into(),
            missing,
        }
    }
}

fn file_separator(rows: &mut Vec<Row>) {
    row(rows, "", false, CONTENT_FILE_GAP);
}

fn row(rows: &mut Vec<Row>, text: impl Into<Arc<str>>, centered: bool, gap: f32) {
    rows.push(Row {
        text: text.into(),
        centered,
        gap,
        blank_line: false,
    });
}

fn blank(rows: &mut Vec<Row>) {
    rows.push(Row {
        text: Arc::from(""),
        centered: true,
        gap: TEXT_ROW_GAP,
        blank_line: true,
    });
}

fn heading(rows: &mut Vec<Row>, text: &str) {
    for line in text.lines() {
        row(rows, line, true, TEXT_ROW_GAP);
    }
}

fn text_rows(rows: &mut Vec<Row>, text: &str) {
    for text in text.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        row(rows, text, false, TEXT_ROW_GAP);
    }
}

fn json_rows(rows: &mut Vec<Row>, value: &serde_json::Value) {
    let Some(sections) = value.as_array() else {
        return;
    };
    for section in sections {
        if let Some(name) = section
            .get("section")
            .and_then(serde_json::Value::as_str)
            .filter(|name| !name.is_empty())
        {
            blank(rows);
            row(rows, "============", true, TEXT_ROW_GAP);
            heading(rows, name);
        }
        if let Some(disciplines) = section
            .get("disciplines")
            .and_then(serde_json::Value::as_array)
        {
            for discipline in disciplines {
                if let Some(name) = discipline
                    .get("discipline")
                    .and_then(serde_json::Value::as_str)
                    .filter(|name| !name.is_empty())
                {
                    blank(rows);
                    heading(rows, name);
                }
                if let Some(titles) = discipline
                    .get("titles")
                    .and_then(serde_json::Value::as_array)
                {
                    for title in titles {
                        if let Some(name) = title
                            .get("title")
                            .and_then(serde_json::Value::as_str)
                            .filter(|name| !name.is_empty())
                        {
                            blank(rows);
                            heading(rows, name);
                        }
                        if let Some(names) =
                            title.get("names").and_then(serde_json::Value::as_array)
                        {
                            for name in names.iter().filter_map(serde_json::Value::as_str) {
                                row(rows, format!("          {name}"), false, TEXT_ROW_GAP);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_document_keeps_poem_paragraphs_order_names_and_quote() {
        let content = Content::read(|path| {
            match path {
            "credits/end.txt" => Some(b"first PLAYERNAME\r\n\r\nsecond".to_vec()),
            "credits/credits.json" => Some(br#"[{"section":"studio","disciplines":[{"discipline":"team","titles":[{"title":"role","names":["one","two"]}]}]}]"#.to_vec()),
            "credits/quote.txt" => Some(b"last".to_vec()),
            _ => None,
        }
        });
        assert!(!content.missing);
        let text = content
            .rows
            .iter()
            .map(|row| row.text.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(&text[1..4], ["first PLAYERNAME", "", "second"]);
        let separators = content
            .rows
            .iter()
            .filter(|row| row.text.is_empty() && row.gap == CONTENT_FILE_GAP)
            .collect::<Vec<_>>();
        assert_eq!(separators.len(), assets::UI_CREDITS_FILES.len());
        assert!(separators.iter().all(|row| !row.blank_line));
        assert_eq!(text.last(), Some(&"last"));
        assert!(
            text.iter()
                .position(|text| *text == "          one")
                .unwrap()
                < text
                    .iter()
                    .position(|text| *text == "          two")
                    .unwrap()
        );
        assert!(
            content
                .rows
                .iter()
                .find(|row| row.text.as_ref() == "role")
                .unwrap()
                .centered
        );
    }

    #[test]
    fn missing_content_is_reported_without_substitute_poem_text() {
        let content = Content::read(|_| None);
        assert!(content.missing);
        assert!(content.rows.is_empty());
    }
}
