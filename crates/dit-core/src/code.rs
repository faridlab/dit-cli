//! The code map (ADR 0025): the import graph of each registered code root,
//! derived from source into the index and queried from it.
//!
//! `refresh_code` is the write side — it reads files at HEAD through
//! `dit-vcs` (I3), parses only the blobs that changed, and resolves every
//! stored import against the files that exist. Everything else here reads
//! the index only (I2).

use std::collections::{HashMap, HashSet, VecDeque};

use dit_index::{StoredCodeMap, StoredMapEntry};
use dit_model::{glob_match, CodeLang, CodeMap, CodeRoot, MapPath};

use crate::{Dit, DitError};

/// Whether a map entry still describes the code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapHealth {
    /// Every path it names exists, and its example is unchanged since the
    /// map was confirmed.
    Holds,
    /// Its paths exist, but nobody has confirmed the map against the code.
    Unconfirmed,
    /// The example changed `commits` times since the map was confirmed.
    Stale { commits: usize },
    /// A path it names matches nothing, or names an unregistered root.
    Broken { reasons: Vec<String> },
}

/// One entry of a code map, judged — what `dit code check` and the agent
/// guide read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapEntryView {
    pub map: String,
    pub path: String,
    pub line: usize,
    pub task: String,
    pub why: Option<String>,
    pub change: Vec<String>,
    pub example: Option<String>,
    pub never: Vec<String>,
    pub health: MapHealth,
}

/// What a refresh did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeReport {
    pub roots: usize,
    /// Files indexed across every root after the refresh.
    pub files: usize,
    /// Files parsed this time — only those whose blob changed.
    pub parsed: usize,
    /// Files dropped: gone from HEAD, or no longer covered.
    pub removed: usize,
    /// Imports that look local but name no file.
    pub unresolved: usize,
    /// Roots that could not be read, with why.
    pub problems: Vec<String>,
}

/// One import of a file, as `dit code uses` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsedImport {
    pub specifier: String,
    pub target: Option<String>,
    pub names: Vec<String>,
    pub external: bool,
    pub reexport: bool,
}

/// What a file depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeUses {
    pub root: String,
    pub path: String,
    pub imports: Vec<UsedImport>,
    /// Names called, deduplicated, in first-use order.
    pub calls: Vec<String>,
}

/// A file that uses a file or a symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeUser {
    pub root: String,
    pub path: String,
    pub names: Vec<String>,
    pub line: u32,
    /// The barrel it came through, when it imported a re-export.
    pub via: Option<String>,
}

/// A much-depended-on file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeHub {
    pub root: String,
    pub path: String,
    /// Distinct files importing it.
    pub users: usize,
}

/// A node, its neighbourhood, and whether it is generated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeExplain {
    pub root: String,
    pub path: String,
    pub generated: bool,
    /// The symbol asked about, when the node was named by a symbol.
    pub symbol: Option<String>,
    pub defines: Vec<String>,
    pub imports: usize,
    pub users: Vec<CodeUser>,
}

/// A symbol or file matching a search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeMatch {
    pub root: String,
    pub path: String,
    /// `None` for a file match.
    pub symbol: Option<String>,
    pub kind: Option<String>,
    pub line: u32,
}

const MAX_REEXPORT_DEPTH: usize = 5;
const EXTRACTOR_KEY: &str = "code-extractor";

impl Dit {
    /// Bring the code map up to HEAD for every registered root. Parses only
    /// files whose blob changed; resolution runs over every stored import,
    /// so an import written before its target existed resolves when it lands.
    pub fn refresh_code(&mut self) -> Result<CodeReport, DitError> {
        if self.index.watermark(EXTRACTOR_KEY)?.as_deref() != Some(dit_code::EXTRACTOR_VERSION) {
            self.invalidate_code_map()?;
            self.index
                .set_watermark(EXTRACTOR_KEY, dit_code::EXTRACTOR_VERSION)?;
        }
        let roots = self.config.code.clone();
        let mut report = CodeReport {
            roots: roots.len(),
            ..CodeReport::default()
        };
        for stale in self.index.code_roots()? {
            if !roots.iter().any(|r| r.id == stale) {
                self.index.remove_code_root(&stale)?;
            }
        }
        for root in &roots {
            if let Err(problem) = self.refresh_root(root, &mut report) {
                report
                    .problems
                    .push(format!("code root `{}`: {problem}", root.id));
            }
        }
        self.judge_code_maps()?;
        Ok(report)
    }

    /// Read one document's `dit-map` fences into the index. Returns how many
    /// were passed over: a fence that cannot name its map, or a second fence
    /// for a map another document already holds.
    pub(crate) fn absorb_map_fences(&mut self, path: &str, text: &str) -> Result<usize, DitError> {
        let mut skipped = 0;
        for fence in dit_parse::codemap::map_fences(text) {
            let (map, problem) = match dit_parse::codemap::parse_code_map(&fence.body) {
                Ok(m) => (m.map, None),
                Err(e) => match dit_parse::codemap::map_in_fence(&fence.body) {
                    Some(name) => (name, Some(e.to_string())),
                    None => {
                        skipped += 1;
                        continue;
                    }
                },
            };
            let kept = self.index.upsert_code_map(&StoredCodeMap {
                map,
                path: path.to_owned(),
                line: fence.line,
                body: fence.body,
                problem,
            })?;
            if !kept {
                skipped += 1;
            }
        }
        Ok(skipped)
    }

    /// Judge every map entry against the code at HEAD: each path against its
    /// root's whole tree, the example against the commit its root was
    /// confirmed at.
    fn judge_code_maps(&mut self) -> Result<(), DitError> {
        let maps = self.index.code_maps()?;
        let mut trees: HashMap<String, Result<HashSet<String>, String>> = HashMap::new();
        for stored in maps {
            let Ok(map) = dit_parse::codemap::parse_code_map(&stored.body) else {
                self.index.replace_code_map_entries(&stored.map, &[])?;
                continue;
            };
            let mut judged = Vec::new();
            for (idx, entry) in map.entries.iter().enumerate() {
                let mut reasons = Vec::new();
                let paths = entry
                    .change
                    .iter()
                    .chain(entry.example.iter())
                    .chain(entry.never.iter());
                for p in paths {
                    if !trees.contains_key(&p.root) {
                        let tree = self.root_tree(&p.root);
                        trees.insert(p.root.clone(), tree);
                    }
                    match &trees[&p.root] {
                        Err(why) => reasons.push(format!("`{}`: {why}", p.written())),
                        Ok(files) => {
                            if !files.iter().any(|f| glob_match(&p.glob, f)) {
                                reasons.push(format!("`{}` matches no file at HEAD", p.written()));
                            }
                        }
                    }
                }
                let health = if reasons.is_empty() {
                    self.example_health(&map, entry.example.as_ref())
                } else {
                    "broken".to_owned()
                };
                judged.push(StoredMapEntry {
                    map: map.map.clone(),
                    idx,
                    task: entry.task.clone(),
                    why: entry.why.clone(),
                    change: entry.change.iter().map(MapPath::written).collect(),
                    example: entry.example.as_ref().map(MapPath::written),
                    never: entry.never.iter().map(MapPath::written).collect(),
                    health,
                    detail: reasons.join("\n"),
                });
            }
            self.index.replace_code_map_entries(&map.map, &judged)?;
        }
        Ok(())
    }

    /// Every file path in a registered root's repository at HEAD.
    fn root_tree(&self, root_id: &str) -> Result<HashSet<String>, String> {
        let Some(root) = self.config.code.iter().find(|r| r.id == root_id) else {
            return Err(format!(
                "`{root_id}` is not a registered code root — add it under `code:`"
            ));
        };
        let repo = self.repo_for(root.repo.as_deref())?;
        let files = repo.get().ls_tree(".").map_err(|e| e.to_string())?;
        Ok(files.into_iter().map(|(p, _)| p).collect())
    }

    /// `holds`, `unconfirmed` or `stale:<n>` for an entry whose paths exist.
    fn example_health(&self, map: &CodeMap, example: Option<&MapPath>) -> String {
        let Some(example) = example else {
            // Nothing to go stale; whether anyone confirmed it still counts.
            let state = if map.confirmed.is_empty() {
                "unconfirmed"
            } else {
                "holds"
            };
            return state.to_owned();
        };
        let Some((_, pin)) = map.confirmed.iter().find(|(r, _)| r == &example.root) else {
            return "unconfirmed".to_owned();
        };
        let Some(root) = self.config.code.iter().find(|r| r.id == example.root) else {
            return "unconfirmed".to_owned();
        };
        let Ok(repo) = self.repo_for(root.repo.as_deref()) else {
            return "unconfirmed".to_owned();
        };
        match repo.get().commits_touching_since(pin, &example.glob) {
            Ok(0) => "holds".to_owned(),
            Ok(n) => format!("stale:{n}"),
            Err(_) => "unconfirmed".to_owned(),
        }
    }

    /// Every judged map entry, from the index (I2).
    pub fn code_map_report(&self) -> Result<Vec<MapEntryView>, DitError> {
        let maps: HashMap<String, StoredCodeMap> = self
            .index
            .code_maps()?
            .into_iter()
            .map(|m| (m.map.clone(), m))
            .collect();
        Ok(self
            .index
            .code_map_entries()?
            .into_iter()
            .map(|e| {
                let at = maps.get(&e.map);
                let health = match e.health.as_str() {
                    "holds" => MapHealth::Holds,
                    "unconfirmed" => MapHealth::Unconfirmed,
                    "broken" => MapHealth::Broken {
                        reasons: e.detail.lines().map(str::to_owned).collect(),
                    },
                    other => MapHealth::Stale {
                        commits: other
                            .strip_prefix("stale:")
                            .and_then(|n| n.parse().ok())
                            .unwrap_or(0),
                    },
                };
                MapEntryView {
                    path: at.map(|m| m.path.clone()).unwrap_or_default(),
                    line: at.map_or(0, |m| m.line),
                    map: e.map,
                    task: e.task,
                    why: e.why,
                    change: e.change,
                    example: e.example,
                    never: e.never,
                    health,
                }
            })
            .collect())
    }

    /// Maps whose fence does not parse, with the reason — `(map, path, line,
    /// problem)`.
    pub fn code_map_problems(&self) -> Result<Vec<(String, String, usize, String)>, DitError> {
        Ok(self
            .index
            .code_maps()?
            .into_iter()
            .filter_map(|m| m.problem.map(|p| (m.map, m.path, m.line, p)))
            .collect())
    }

    /// A person has read the map and agrees it still holds: pin each root it
    /// names to that root's HEAD, in one commit. The pin never moves on its
    /// own — the same stance as `dit morse sync` (ADR 0022).
    pub fn code_map_confirm(
        &mut self,
        map: &str,
        author: &str,
    ) -> Result<Vec<(String, String)>, DitError> {
        let stored = self
            .index
            .code_maps()?
            .into_iter()
            .find(|m| m.map == map)
            .ok_or_else(|| DitError::NotFound(format!("map `{map}`")))?;
        let parsed = dit_parse::codemap::parse_code_map(&stored.body)
            .map_err(|e| DitError::Refuse(format!("map `{map}`: {e}")))?;
        // Confirming says "this map holds"; a map naming a path that is gone
        // does not, so the pin would be a false claim.
        let broken: Vec<String> = self
            .index
            .code_map_entries()?
            .into_iter()
            .filter(|e| e.map == map && e.health == "broken")
            .map(|e| e.task)
            .collect();
        if !broken.is_empty() {
            return Err(DitError::Refuse(format!(
                "map `{map}` has broken entries — fix them first (`dit code check`): {}",
                broken.join("; ")
            )));
        }
        let mut roots: Vec<String> = parsed
            .entries
            .iter()
            .flat_map(|e| {
                e.change
                    .iter()
                    .chain(e.example.iter())
                    .chain(e.never.iter())
            })
            .map(|p| p.root.clone())
            .collect();
        roots.sort();
        roots.dedup();
        let mut pins = Vec::new();
        for root_id in roots {
            let Some(root) = self.config.code.iter().find(|r| r.id == root_id) else {
                return Err(DitError::Refuse(format!(
                    "map `{map}` names `{root_id}`, which is not a registered code root"
                )));
            };
            let repo = self
                .repo_for(root.repo.as_deref())
                .map_err(DitError::Refuse)?;
            let head = repo
                .get()
                .head()
                .map_err(|e| DitError::Refuse(format!("`{root_id}` has no HEAD: {e}")))?;
            pins.push((root_id, head));
        }
        let body = self.read_doc(&stored.path)?;
        let updated = repin_map(&body, map, &pins).ok_or_else(|| {
            DitError::Refuse(format!(
                "the fence of map `{map}` could not be found in {}",
                stored.path
            ))
        })?;
        let mut tx = self.transaction(author)?;
        tx.write_doc(&stored.path, &updated)?;
        tx.commit(&format!(
            "dit code map confirm {map}: checked against the code at HEAD"
        ))?;
        Ok(pins)
    }

    /// Drop the whole code map, so the next refresh reads every file again —
    /// after an extractor change, or on `dit code refresh --full`.
    pub fn invalidate_code_map(&mut self) -> Result<(), DitError> {
        for root in self.index.code_roots()? {
            self.index.remove_code_root(&root)?;
        }
        self.index.set_watermark(EXTRACTOR_KEY, "")?;
        Ok(())
    }

    fn refresh_root(&mut self, root: &CodeRoot, report: &mut CodeReport) -> Result<(), String> {
        let repo_ref = self.repo_for(root.repo.as_deref())?;
        let repo = repo_ref.get();
        let listed = repo.ls_tree(".").map_err(|e| e.to_string())?;
        // Every code file in the repository is a possible import target —
        // including excluded ones — so an import into generated code resolves
        // to its path even though that file is not itself indexed.
        let code_files: HashSet<String> = listed
            .iter()
            .filter(|(p, _)| CodeLang::of(p).is_some())
            .map(|(p, _)| p.clone())
            .collect();
        let covered: Vec<(String, String)> = listed
            .into_iter()
            .filter(|(p, _)| CodeLang::of(p).is_some() && root.covers(p))
            .collect();
        let aliases = tsconfig_aliases(repo.show_text("HEAD:tsconfig.json").as_deref());

        // Read what changed in one batch, then parse it across threads —
        // outside the index borrow.
        let known = self.index.code_blobs(&root.id).map_err(|e| e.to_string())?;
        let changed: Vec<(String, String, CodeLang)> = covered
            .iter()
            .filter(|(path, blob)| known.get(path) != Some(blob))
            .filter_map(|(path, blob)| CodeLang::of(path).map(|l| (path.clone(), blob.clone(), l)))
            .collect();
        let shas: Vec<String> = changed.iter().map(|(_, b, _)| b.clone()).collect();
        let texts = repo.read_blobs(&shas).map_err(|e| e.to_string())?;
        drop(repo_ref);
        let work: Vec<(String, String, CodeLang, String)> = changed
            .into_iter()
            .zip(texts)
            .filter_map(|((path, blob, lang), text)| text.map(|t| (path, blob, lang, t)))
            .collect();
        let parsed = parse_all(work);

        let keep: HashSet<&str> = covered.iter().map(|(p, _)| p.as_str()).collect();
        for path in known.keys() {
            if !keep.contains(path.as_str()) {
                self.index
                    .remove_code_file(&root.id, path)
                    .map_err(|e| e.to_string())?;
                report.removed += 1;
            }
        }
        for (path, blob, lang, facts) in &parsed {
            self.index
                .replace_code_file(
                    &root.id,
                    path,
                    blob,
                    lang.as_str(),
                    root.is_generated(path),
                    facts,
                )
                .map_err(|e| e.to_string())?;
        }
        report.parsed += parsed.len();
        report.files += covered.len();

        // Resolve every import against the files that exist now.
        let index = dit_code::RootIndex {
            files: &code_files,
            aliases: &aliases,
        };
        let imports = self
            .index
            .code_imports_of_root(&root.id)
            .map_err(|e| e.to_string())?;
        let mut targets = Vec::new();
        for imp in &imports {
            let lang = match imp.lang.as_str() {
                "rust" => CodeLang::Rust,
                "typescript" => CodeLang::TypeScript,
                _ => CodeLang::Tsx,
            };
            let (target, external) =
                match dit_code::resolve(lang, &imp.path, &imp.specifier, &index) {
                    dit_code::Resolved::File(f) => (Some(f), false),
                    dit_code::Resolved::External => (None, true),
                    dit_code::Resolved::Unresolved => {
                        report.unresolved += 1;
                        (None, false)
                    }
                };
            if imp.target != target || imp.external != external {
                targets.push((imp.id, target, external));
            }
        }
        self.index
            .set_code_import_targets(&targets)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Where a name points: an exact indexed path, the files defining a
    /// symbol of that name, or files whose path contains it — in that order.
    fn locate(&self, name: &str) -> Result<Located, DitError> {
        for root in self.index.code_roots()? {
            if self.index.code_blobs(&root)?.contains_key(name) {
                return Ok(Located::File(root, name.to_owned()));
            }
        }
        let symbols = self.index.code_symbols_named(name)?;
        if !symbols.is_empty() {
            let mut files: Vec<(String, String)> =
                symbols.into_iter().map(|s| (s.root, s.path)).collect();
            files.dedup();
            return Ok(Located::Symbol(name.to_owned(), files));
        }
        let mut near = self.index.code_files_like(name, 1)?;
        match near.pop() {
            Some((root, path)) => Ok(Located::File(root, path)),
            None => Err(DitError::NotFound(format!(
                "`{name}` names no indexed file or symbol — run `dit code where {name}`"
            ))),
        }
    }

    /// What a file (or the file defining a symbol) imports and calls.
    pub fn code_uses(&self, name: &str) -> Result<CodeUses, DitError> {
        let (root, path) = match self.locate(name)? {
            Located::File(root, path) => (root, path),
            Located::Symbol(_, mut files) => files.remove(0),
        };
        let imports = self
            .index
            .code_imports_of(&root, &path)?
            .into_iter()
            .map(|i| UsedImport {
                specifier: i.specifier,
                target: i.target,
                names: i.names,
                external: i.external,
                reexport: i.reexport,
            })
            .collect();
        let mut seen = HashSet::new();
        let calls = self
            .index
            .code_calls_of(&root, &path)?
            .into_iter()
            .map(|c| c.callee)
            .filter(|c| seen.insert(c.clone()))
            .collect();
        Ok(CodeUses {
            root,
            path,
            imports,
            calls,
        })
    }

    /// Who uses a file (its importers) or a symbol (importers naming it,
    /// followed through re-exporting barrels).
    pub fn code_users(&self, name: &str) -> Result<Vec<CodeUser>, DitError> {
        match self.locate(name)? {
            Located::File(root, path) => {
                let mut out = Vec::new();
                let mut seen = HashSet::new();
                self.file_users(&root, &path, None, None, 0, &mut seen, &mut out)?;
                Ok(out)
            }
            Located::Symbol(symbol, files) => {
                let mut out = Vec::new();
                let mut seen = HashSet::new();
                for (root, path) in files {
                    self.symbol_users(&root, &path, &symbol, None, 0, &mut seen, &mut out)?;
                }
                Ok(out)
            }
        }
    }

    /// Importers of a file; a barrel re-exporting from it is a hop, not a
    /// user — its own importers are, when they take a name it passes on
    /// (`names` narrows what each hop re-exports; `None` means everything).
    #[allow(clippy::too_many_arguments)]
    fn file_users(
        &self,
        root: &str,
        file: &str,
        names: Option<&[String]>,
        via: Option<&str>,
        depth: usize,
        seen: &mut HashSet<(String, String)>,
        out: &mut Vec<CodeUser>,
    ) -> Result<(), DitError> {
        if depth > MAX_REEXPORT_DEPTH {
            return Ok(());
        }
        for imp in self.index.code_importers(root, file)? {
            let takes = match names {
                None => true,
                Some(passed) => imp
                    .names
                    .iter()
                    .any(|n| n == "*" || passed.iter().any(|p| p == "*" || p == n)),
            };
            if !takes || !seen.insert((imp.root.clone(), imp.path.clone())) {
                continue;
            }
            if imp.reexport {
                let passed: Vec<String> = imp.names.clone();
                let narrowed: Vec<String> = match names {
                    // Keep only what both hops pass on.
                    Some(outer) if !passed.iter().any(|p| p == "*") => passed
                        .into_iter()
                        .filter(|p| outer.iter().any(|o| o == "*" || o == p))
                        .collect(),
                    Some(outer) => outer.to_vec(),
                    None => passed,
                };
                self.file_users(
                    root,
                    &imp.path,
                    Some(&narrowed),
                    Some(&imp.path),
                    depth + 1,
                    seen,
                    out,
                )?;
            } else {
                out.push(CodeUser {
                    root: imp.root,
                    path: imp.path,
                    names: imp.names,
                    line: imp.line,
                    via: via.map(str::to_owned),
                });
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn symbol_users(
        &self,
        root: &str,
        file: &str,
        symbol: &str,
        via: Option<&str>,
        depth: usize,
        seen: &mut HashSet<(String, String)>,
        out: &mut Vec<CodeUser>,
    ) -> Result<(), DitError> {
        if depth > MAX_REEXPORT_DEPTH {
            return Ok(());
        }
        for imp in self.index.code_importers(root, file)? {
            let takes = imp.names.iter().any(|n| n == symbol || n == "*");
            if !takes || !seen.insert((imp.root.clone(), imp.path.clone())) {
                continue;
            }
            if imp.reexport {
                // A barrel passing it on: its own importers are the users.
                self.symbol_users(
                    root,
                    &imp.path,
                    symbol,
                    Some(&imp.path),
                    depth + 1,
                    seen,
                    out,
                )?;
            } else {
                out.push(CodeUser {
                    root: imp.root,
                    path: imp.path,
                    names: imp.names,
                    line: imp.line,
                    via: via.map(str::to_owned),
                });
            }
        }
        Ok(())
    }

    /// The shortest import chain from `from` to `to`, both named like any
    /// node. Empty when there is none.
    pub fn code_path(&self, from: &str, to: &str) -> Result<Vec<String>, DitError> {
        let (root, start) = self.file_of(from)?;
        let (_, goal) = self.file_of(to)?;
        let mut next: HashMap<String, Vec<String>> = HashMap::new();
        for (a, b) in self.index.code_edges(&root)? {
            next.entry(a).or_default().push(b);
        }
        let mut prev: HashMap<String, String> = HashMap::new();
        let mut queue = VecDeque::from([start.clone()]);
        let mut seen = HashSet::from([start.clone()]);
        while let Some(node) = queue.pop_front() {
            if node == goal {
                let mut chain = vec![goal.clone()];
                let mut at = goal.clone();
                while let Some(p) = prev.get(&at) {
                    chain.push(p.clone());
                    at = p.clone();
                }
                chain.reverse();
                return Ok(chain);
            }
            for n in next.get(&node).into_iter().flatten() {
                if seen.insert(n.clone()) {
                    prev.insert(n.clone(), node.clone());
                    queue.push_back(n.clone());
                }
            }
        }
        Ok(Vec::new())
    }

    fn file_of(&self, name: &str) -> Result<(String, String), DitError> {
        Ok(match self.locate(name)? {
            Located::File(root, path) => (root, path),
            Located::Symbol(_, mut files) => files.remove(0),
        })
    }

    /// The most-imported files, per root or across every root.
    pub fn code_hubs(
        &self,
        root: Option<&str>,
        limit: usize,
        generated: bool,
    ) -> Result<Vec<CodeHub>, DitError> {
        Ok(self
            .index
            .code_hubs(root, limit, generated)?
            .into_iter()
            .map(|(root, path, users)| CodeHub { root, path, users })
            .collect())
    }

    /// A node and its neighbourhood.
    pub fn code_explain(&self, name: &str) -> Result<CodeExplain, DitError> {
        let (root, path, symbol) = match self.locate(name)? {
            Located::File(root, path) => (root, path, None),
            Located::Symbol(symbol, mut files) => {
                let (root, path) = files.remove(0);
                (root, path, Some(symbol))
            }
        };
        let generated = self
            .index
            .code_file_generated(&root, &path)?
            .unwrap_or(false);
        let defines = self
            .index
            .code_symbols_of(&root, &path)?
            .into_iter()
            .filter(|s| s.exported)
            .map(|s| s.name)
            .collect();
        let imports = self.index.code_imports_of(&root, &path)?.len();
        let users = match &symbol {
            Some(s) => self.code_users(s)?,
            None => self.code_users(&path)?,
        };
        Ok(CodeExplain {
            root,
            path,
            generated,
            symbol,
            defines,
            imports,
            users,
        })
    }

    /// Symbols and files whose name contains `text`.
    pub fn code_where(&self, text: &str, limit: usize) -> Result<Vec<CodeMatch>, DitError> {
        let mut out: Vec<CodeMatch> = self
            .index
            .code_symbols_like(text, limit)?
            .into_iter()
            .map(|s| CodeMatch {
                root: s.root,
                path: s.path,
                symbol: Some(s.name),
                kind: Some(s.kind),
                line: s.line,
            })
            .collect();
        for (root, path) in self.index.code_files_like(text, limit)? {
            out.push(CodeMatch {
                root,
                path,
                symbol: None,
                kind: None,
                line: 0,
            });
        }
        out.truncate(limit);
        Ok(out)
    }
}

/// Set the `confirmed:` line of one map's fence, inserting it after `map:`
/// when absent, and leave every other byte of the document alone.
fn repin_map(document: &str, map: &str, pins: &[(String, String)]) -> Option<String> {
    let pairs: Vec<String> = pins.iter().map(|(r, c)| format!("{r}: {c}")).collect();
    let line_text = format!("confirmed: {{ {} }}\n", pairs.join(", "));
    let lines: Vec<&str> = document.split_inclusive('\n').collect();
    let mut open: Option<usize> = None;
    let mut ours = false;
    for (i, raw) in lines.iter().enumerate() {
        let t = raw.trim();
        if t.starts_with("```") {
            match open {
                Some(start) => {
                    if ours {
                        let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
                        let inside = start + 1..i;
                        if let Some(at) =
                            inside.clone().find(|&j| lines[j].starts_with("confirmed:"))
                        {
                            out[at] = line_text;
                        } else {
                            let at = inside.clone().find(|&j| lines[j].starts_with("map:"))?;
                            out.insert(at + 1, line_text);
                        }
                        return Some(out.concat());
                    }
                    open = None;
                }
                None => {
                    if t.trim_start_matches('`').trim() == dit_parse::codemap::MAP_FENCE {
                        open = Some(i);
                        ours = false;
                    }
                }
            }
            continue;
        }
        if open.is_some() {
            if let Some(rest) = t.strip_prefix("map:") {
                ours = rest.trim().trim_matches('"').trim_matches('\'') == map;
            }
        }
    }
    None
}

/// Parse files across the machine's cores; each thread keeps its own parser.
fn parse_all(
    work: Vec<(String, String, CodeLang, String)>,
) -> Vec<(String, String, CodeLang, dit_model::FileFacts)> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 16);
    if work.len() < 64 || threads == 1 {
        return work
            .into_iter()
            .map(|(p, b, l, t)| {
                let facts = dit_code::extract(l, &t);
                (p, b, l, facts)
            })
            .collect();
    }
    let chunk = work.len().div_ceil(threads);
    let chunks: Vec<Vec<_>> = {
        let mut rest = work;
        let mut out = Vec::new();
        while !rest.is_empty() {
            let tail = rest.split_off(chunk.min(rest.len()));
            out.push(rest);
            rest = tail;
        }
        out
    };
    std::thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .into_iter()
                        .map(|(p, b, l, t)| {
                            let facts = dit_code::extract(l, &t);
                            (p, b, l, facts)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    })
}

enum Located {
    File(String, String),
    Symbol(String, Vec<(String, String)>),
}

/// `compilerOptions.paths` from a tsconfig, as `(pattern, target)` pairs
/// with the leading `./` dropped. tsconfig allows comments and trailing
/// commas, so both are stripped before parsing; a file that still does not
/// parse yields no aliases rather than an error.
fn tsconfig_aliases(text: Option<&str>) -> Vec<(String, String)> {
    let Some(text) = text else {
        return Vec::new();
    };
    let cleaned: String = text
        .lines()
        .map(|l| match l.find("//") {
            Some(i) if !l[..i].contains('"') || l[..i].matches('"').count() % 2 == 0 => &l[..i],
            _ => l,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let cleaned = strip_trailing_commas(&cleaned);
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&cleaned) else {
        return Vec::new();
    };
    let base = json["compilerOptions"]["baseUrl"]
        .as_str()
        .unwrap_or(".")
        .trim_start_matches("./")
        .trim_end_matches('/')
        .trim_start_matches('.');
    let Some(paths) = json["compilerOptions"]["paths"].as_object() else {
        return Vec::new();
    };
    paths
        .iter()
        .filter_map(|(pattern, targets)| {
            let target = targets.as_array()?.first()?.as_str()?;
            let target = target.trim_start_matches("./");
            let target = if base.is_empty() {
                target.to_owned()
            } else {
                format!("{base}/{target}")
            };
            Some((pattern.clone(), target))
        })
        .collect()
}

/// Drop a comma that only a closing `}` or `]` follows, outside strings.
fn strip_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    for (i, &c) in chars.iter().enumerate() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|n| !n.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn tsconfig_paths_become_aliases_through_comments_and_base_url() {
        let text = "{\n  // a comment\n  \"compilerOptions\": {\n    \"baseUrl\": \".\",\n    \"paths\": { \"@/*\": [\"./src/*\"], \"~lib\": [\"lib/index.ts\"], },\n  }\n}\n";
        let aliases = tsconfig_aliases(Some(text));
        assert!(
            aliases.contains(&("@/*".to_owned(), "src/*".to_owned())),
            "{aliases:?}"
        );
        assert!(aliases.contains(&("~lib".to_owned(), "lib/index.ts".to_owned())));
        assert!(tsconfig_aliases(Some("not json")).is_empty());
        let based = "{ \"compilerOptions\": { \"baseUrl\": \"./app\", \"paths\": { \"@/*\": [\"src/*\"] } } }";
        assert_eq!(
            tsconfig_aliases(Some(based)),
            vec![("@/*".to_owned(), "app/src/*".to_owned())]
        );
    }
}
