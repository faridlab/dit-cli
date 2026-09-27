//! The `dit-map` fence (ADR 0025): authored intent about a codebase — where a
//! task is done, the file to copy, the paths never to touch — pinned to the
//! code roots it names. Parsing only; judging it against the code is the
//! facade's, at refresh.

use dit_model::{CodeMap, MapEntry, MapPath};

use crate::flowshape::{fences, Fence};
use crate::yaml::{self, Yaml};

pub const MAP_FENCE: &str = "dit-map";

/// A map fence that cannot be read, named so the author can fix it.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum MapError {
    #[error(transparent)]
    Yaml(#[from] yaml::YamlError),
    #[error("the fence has no `map:` — a map must say what it is called")]
    NoMap,
    #[error("`entries:` must be a list of entries")]
    NotAList,
    #[error("entry {index} needs a `task:` and at least one of `change:`, `example:` or `never:`")]
    BadEntry { index: usize },
    #[error("entry {index}: `{found}` must name a code root and a path, as `<root>:<path>`")]
    BadPath { index: usize, found: String },
    #[error("`{0}` is not a key a DIT file may carry: a field naming something to run or fetch is remote code execution by pull request")]
    Forbidden(String),
}

/// The map fences in a document, with the line each one starts on.
pub fn map_fences(body: &str) -> Vec<Fence> {
    fences(body)
        .into_iter()
        .filter(|f| f.info == MAP_FENCE)
        .collect()
}

/// The `map:` a fence names, even when the rest does not parse.
pub fn map_in_fence(body: &str) -> Option<String> {
    body.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("map:")?;
        let name = rest.trim().trim_matches('"').trim_matches('\'');
        (!name.is_empty()).then(|| name.to_owned())
    })
}

pub fn parse_code_map(text: &str) -> Result<CodeMap, MapError> {
    let root = yaml::parse(text)?;
    refuse(&root)?;
    let map = root
        .get("map")
        .and_then(Yaml::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(MapError::NoMap)?
        .to_owned();
    let confirmed = match root.get("confirmed") {
        Some(Yaml::Map(pairs)) => pairs
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|c| (k.clone(), c.trim().to_owned())))
            .filter(|(_, c)| !c.is_empty())
            .collect(),
        _ => Vec::new(),
    };
    let mut entries = Vec::new();
    if let Some(node) = root.get("entries") {
        for (index, item) in node.as_seq().ok_or(MapError::NotAList)?.iter().enumerate() {
            entries.push(entry(item, index)?);
        }
    }
    Ok(CodeMap {
        map,
        entries,
        confirmed,
    })
}

fn strings(node: Option<&Yaml>) -> Vec<String> {
    match node {
        Some(Yaml::Seq(items)) => items
            .iter()
            .filter_map(Yaml::as_str)
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect(),
        Some(Yaml::Str(one)) if !one.trim().is_empty() => vec![one.trim().to_owned()],
        _ => Vec::new(),
    }
}

fn entry(item: &Yaml, index: usize) -> Result<MapEntry, MapError> {
    let path = |text: String| {
        MapPath::parse(&text).ok_or(MapError::BadPath {
            index,
            found: text.clone(),
        })
    };
    let task = item
        .get("task")
        .and_then(Yaml::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let change = strings(item.get("change"))
        .into_iter()
        .map(path)
        .collect::<Result<Vec<_>, _>>()?;
    let never = strings(item.get("never"))
        .into_iter()
        .map(path)
        .collect::<Result<Vec<_>, _>>()?;
    let example = strings(item.get("example"))
        .into_iter()
        .next()
        .map(path)
        .transpose()?;
    let why = item
        .get("why")
        .and_then(Yaml::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let Some(task) = task else {
        return Err(MapError::BadEntry { index });
    };
    if change.is_empty() && example.is_none() && never.is_empty() {
        return Err(MapError::BadEntry { index });
    }
    Ok(MapEntry {
        task,
        change,
        example,
        never,
        why,
    })
}

fn refuse(node: &Yaml) -> Result<(), MapError> {
    match node {
        Yaml::Map(entries) => {
            for (key, value) in entries {
                if crate::morse::FORBIDDEN_KEYS.contains(&key.to_ascii_lowercase().as_str()) {
                    return Err(MapError::Forbidden(key.clone()));
                }
                refuse(value)?;
            }
            Ok(())
        }
        Yaml::Seq(items) => items.iter().try_for_each(refuse),
        _ => Ok(()),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const MAP: &str = r#"map: webapp
confirmed: { web: a3f9c2d }
entries:
  - task: "Customise how one entity is listed or edited"
    change: [ "web:src/resources/*/index.ts" ]
    example: "web:src/resources/product/index.ts"
    never: [ "web:src/generated/**" ]
    why: "The generic engine renders every entity; its config lives in its folder."
  - task: A People desk screen with verbs
    example: web:src/desks/people/PayrollRunsPage.tsx
    change: [ "web:src/desks/people/", "web:src/routes.tsx" ]
"#;

    #[test]
    fn a_map_carries_its_entries_paths_and_pin() {
        let m = parse_code_map(MAP).unwrap();
        assert_eq!(m.map, "webapp");
        assert_eq!(m.confirmed, vec![("web".to_owned(), "a3f9c2d".to_owned())]);
        assert_eq!(m.entries.len(), 2);
        let first = &m.entries[0];
        assert_eq!(first.task, "Customise how one entity is listed or edited");
        assert_eq!(first.change[0].written(), "web:src/resources/*/index.ts");
        assert_eq!(
            first.example.as_ref().unwrap().glob,
            "src/resources/product/index.ts"
        );
        assert_eq!(first.never[0].glob, "src/generated/**");
        assert!(first
            .why
            .as_deref()
            .unwrap()
            .starts_with("The generic engine"));
        assert_eq!(m.entries[1].change.len(), 2);
        assert!(m.entries[1].why.is_none());
    }

    #[test]
    fn a_path_without_its_root_is_refused_by_name() {
        let bad = MAP.replace("\"web:src/routes.tsx\"", "\"src/routes.tsx\"");
        let err = parse_code_map(&bad).unwrap_err();
        assert!(
            matches!(&err, MapError::BadPath { found, .. } if found == "src/routes.tsx"),
            "{err:?}"
        );
    }

    #[test]
    fn a_map_that_names_something_to_run_is_refused() {
        let bad = MAP.replace(
            "    why: \"The generic",
            "    run: \"make\"\n    why: \"The generic",
        );
        assert!(matches!(parse_code_map(&bad).unwrap_err(), MapError::Forbidden(k) if k == "run"));
    }

    #[test]
    fn an_entry_without_a_task_or_anything_to_point_at_is_refused() {
        let bad = "map: m\nentries:\n  - why: nothing\n";
        assert!(matches!(
            parse_code_map(bad).unwrap_err(),
            MapError::BadEntry { index: 0 }
        ));
    }

    #[test]
    fn fences_are_found_in_a_document() {
        let doc = format!("# Where things live\n\n```dit-map\n{MAP}```\n");
        let found = map_fences(&doc);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 3);
        assert_eq!(map_in_fence(&found[0].body).as_deref(), Some("webapp"));
    }
}
