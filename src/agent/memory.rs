use crate::{Ctx, Reply};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_ITEMS: usize = 1000;
const MAX_CONTENT: usize = 8192;
const MAX_TAGS: usize = 20;
const MAX_TAG: usize = 64;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Memory {
    id: String,
    content: String,
    tags: Vec<String>,
    created_at: u64,
    updated_at: u64,
}

fn arg<'a>(a: &'a Value, key: &str) -> Result<&'a str, String> {
    a[key].as_str().ok_or_else(|| format!("缺少参数 {key}"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn next_id() -> String {
    format!("{}-{}", now(), NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

fn clean_content(value: &str) -> Result<String, String> {
    let content = value.trim();
    if content.is_empty() {
        return Err("记忆内容不能为空".into());
    }
    if content.chars().count() > MAX_CONTENT {
        return Err(format!("记忆内容不能超过 {MAX_CONTENT} 字符"));
    }
    Ok(content.to_owned())
}

fn clean_tags(value: Option<&Value>) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let list = value.as_array().ok_or("tags 必须是字符串数组")?;
    if list.len() > MAX_TAGS {
        return Err(format!("标签不能超过 {MAX_TAGS} 个"));
    }
    let mut tags: Vec<String> = Vec::new();
    for item in list {
        let tag = item.as_str().ok_or("标签必须是字符串")?.trim();
        if tag.is_empty() || tag.chars().count() > MAX_TAG {
            return Err(format!("标签长度必须在 1–{MAX_TAG} 字符之间"));
        }
        if !tags.iter().any(|t| t.as_str() == tag) {
            tags.push(tag.to_owned());
        }
    }
    Ok(tags)
}

fn entries(value: &Value) -> Result<Vec<Memory>, String> {
    let list = value.as_array().ok_or("agent-memory.json 格式错误")?;
    let list: Vec<Memory> =
        serde_json::from_value(Value::Array(list.clone())).map_err(|e| e.to_string())?;
    if list.len() > MAX_ITEMS {
        return Err("记忆条目过多".into());
    }
    Ok(list)
}

fn terms(query: &str) -> Vec<String> {
    let query = query.trim().to_lowercase();
    let mut terms = vec![query.clone()];
    terms.extend(query.split_whitespace().map(str::to_owned));
    let chars = query.chars().collect::<Vec<_>>();
    for pair in chars.windows(2) {
        if pair.iter().all(|c| is_cjk(*c)) {
            terms.push(pair.iter().collect());
        }
    }
    let mut seen = std::collections::HashSet::new();
    terms
        .into_iter()
        .filter(|term| !term.is_empty())
        .filter(|term| seen.insert(term.clone()))
        .take(32)
        .collect()
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0x3040..=0x30ff | 0xac00..=0xd7af)
}

pub fn add(ctx: &Ctx, a: &Value) -> Reply {
    let entry = Memory {
        id: next_id(),
        content: clean_content(arg(a, "content")?)?,
        tags: clean_tags(a.get("tags"))?,
        created_at: now(),
        updated_at: now(),
    };
    ctx.update("agent-memory", json!([]), |value| {
        let mut list = entries(value)?;
        if list.len() >= MAX_ITEMS {
            return Err(format!("最多保存 {MAX_ITEMS} 条记忆"));
        }
        list.push(entry.clone());
        *value = json!(list);
        Ok(())
    })?;
    Ok(json!(entry))
}

pub fn search(ctx: &Ctx, a: &Value) -> Reply {
    let query = arg(a, "query")?.trim().to_lowercase();
    let limit = a["limit"].as_u64().unwrap_or(5).clamp(1, 20) as usize;
    let mut list = entries(&ctx.read("agent-memory", json!([]))?)?;
    if query.is_empty() {
        list.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at));
        return Ok(json!(list.into_iter().take(limit).collect::<Vec<_>>()));
    }
    let terms = terms(&query);
    let mut scored = list
        .into_iter()
        .filter_map(|entry| {
            let content = entry.content.to_lowercase();
            let tags = entry.tags.join("\n").to_lowercase();
            let mut score = if content.contains(&query) { 12 } else { 0 };
            for term in &terms {
                if term == &query {
                    continue;
                }
                score += content.matches(term).count() * 2;
                score += tags.matches(term).count() * 5;
            }
            (score > 0).then_some((score, entry))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|(a, x), (b, y)| b.cmp(a).then_with(|| y.updated_at.cmp(&x.updated_at)));
    Ok(json!(scored
        .into_iter()
        .take(limit)
        .map(|(_, entry)| entry)
        .collect::<Vec<_>>()))
}

pub fn update(ctx: &Ctx, a: &Value) -> Reply {
    let id = arg(a, "id")?;
    let content = match a.get("content") {
        None => None,
        Some(Value::String(content)) => Some(clean_content(content)?),
        Some(_) => return Err("content 必须是字符串".into()),
    };
    let tags = clean_tags(a.get("tags"))?;
    let has_tags = a.get("tags").is_some();
    let mut updated = None;
    ctx.update("agent-memory", json!([]), |value| {
        let mut list = entries(value)?;
        let entry = list
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or("没有这条记忆")?;
        if let Some(content) = &content {
            entry.content = content.clone();
        }
        if has_tags {
            entry.tags = tags.clone();
        }
        entry.updated_at = now();
        updated = Some(entry.clone());
        *value = json!(list);
        Ok(())
    })?;
    Ok(json!(updated.ok_or("没有这条记忆")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_searches_and_updates_memory() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(temp.path().join("data")).unwrap();
        let added = add(
            &ctx,
            &json!({"content":"用户偏好中文回答","tags":["偏好","语言"]}),
        )
        .unwrap();
        let id = added["id"].as_str().unwrap();
        assert_eq!(search(&ctx, &json!({"query":"中文"})).unwrap()[0]["id"], id);
        let updated = update(&ctx, &json!({"id":id,"content":"用户偏好简洁中文回答"})).unwrap();
        assert_eq!(updated["tags"], json!(["偏好", "语言"]));
        assert_eq!(search(&ctx, &json!({"query":"简洁"})).unwrap()[0]["id"], id);
    }

    #[test]
    fn rejects_invalid_memory_content() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(temp.path().join("data")).unwrap();
        assert!(add(&ctx, &json!({"content":"   "})).is_err());
        assert!(add(&ctx, &json!({"content":"x","tags":"not-a-list"})).is_err());
    }
}
