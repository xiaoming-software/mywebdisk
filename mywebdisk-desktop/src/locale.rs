use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Zh,
}

pub struct Locale {
    en: HashMap<String, String>,
    zh: HashMap<String, String>,
    pub lang: Lang,
}

impl Locale {
    /// 编译期嵌入文案，.app / .exe 无需附带 csv 文件。
    pub fn load_embedded() -> Self {
        Self::parse(include_str!("../locale/ui.csv"))
    }

    fn parse(text: &str) -> Self {
        let mut en = HashMap::new();
        let mut zh = HashMap::new();
        let mut header = true;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if header {
                header = false;
                continue;
            }
            if let Some((key, en_val, zh_val)) = parse_row(line) {
                en.insert(key.clone(), en_val);
                zh.insert(key, zh_val);
            }
        }
        Self {
            en,
            zh,
            lang: Lang::En,
        }
    }

    pub fn tr(&self, key: &str) -> String {
        let map = match self.lang {
            Lang::En => &self.en,
            Lang::Zh => &self.zh,
        };
        map.get(key)
            .cloned()
            .unwrap_or_else(|| key.to_string())
    }

}

fn parse_row(line: &str) -> Option<(String, String, String)> {
    let first = line.find(',')?;
    let second = line[first + 1..].find(',')? + first + 1;
    Some((
        line[..first].to_string(),
        line[first + 1..second].to_string(),
        line[second + 1..].to_string(),
    ))
}
