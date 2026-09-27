//! The code map (ADR 0025): the import graph of each registered code root,
//! derived from source into the index and queried from it.
//!
//! `refresh_code` is the write side — it reads files at HEAD through
//! `dit-vcs` (I3), parses only the blobs that changed, and resolves every
//! stored import against the files that exist. Everything else here reads
//! the index only (I2).

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use dit_index::{KotlinReferences, StoredCodeMap, StoredMapEntry};
use dit_model::{glob_match, CodeLang, CodeMap, CodeRoot, FileFacts, MapPath};

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

/// An operation a literal calls, and where it was proven.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiOperation {
    pub spec: String,
    pub operation_id: String,
    pub method: String,
    pub path: String,
    /// Environments where a scenario exercising it holds a fresh proof.
    pub proven: Vec<String>,
}

/// One path literal in the code, read against the registered specs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiCall {
    pub root: String,
    pub path: String,
    pub line: u32,
    /// As written, in the `${name}` form.
    pub written: String,
    /// With every constant it names put back in.
    pub resolved: String,
    /// Empty for an orphan: no registered spec describes it.
    pub operations: Vec<ApiOperation>,
}

/// The seam link (ADR 0025 milestone 3): every literal that looks like a
/// call into a registered spec, matched or not.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApiReport {
    pub calls: Vec<ApiCall>,
    /// The first path segments of the registered specs — a literal must
    /// start with one to count as a call at all.
    pub roots: Vec<String>,
    pub operations: usize,
    /// Literals too generic to name one path — mostly wildcards, like a
    /// CRUD client's `${base}/${module}/${collection}` — left out.
    pub generic: usize,
}

impl ApiReport {
    pub fn orphans(&self) -> impl Iterator<Item = &ApiCall> {
        self.calls.iter().filter(|c| c.operations.is_empty())
    }

    /// Operations with callers and no fresh proof anywhere, each once.
    pub fn unproven(&self) -> Vec<&ApiOperation> {
        let mut seen = HashSet::new();
        self.calls
            .iter()
            .flat_map(|c| c.operations.iter())
            .filter(|o| o.proven.is_empty())
            .filter(|o| seen.insert((o.spec.clone(), o.operation_id.clone())))
            .collect()
    }
}

/// A literal whose closest fit still fills more spec-named segments than
/// this with values is not naming a path; it is a template for many.
const MAX_GUESSED: usize = 1;

/// How deep a constant may be built from other constants.
const MAX_CONST_DEPTH: usize = 4;

/// What a refresh did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeReport {
    pub roots: usize,
    /// Files indexed across every root after the refresh.
    pub files: usize,
    /// Files parsed this time — only those whose blob changed and was never
    /// read before.
    pub parsed: usize,
    /// Files whose blob changed but was read before — on another branch, at
    /// another path — and came from the cache instead of the parser.
    pub reused: usize,
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
    /// How many of the search words the file answers, by its path or the
    /// names it defines.
    pub terms: usize,
    pub generated: bool,
    /// The matching symbols it defines, with their lines, best first.
    pub symbols: Vec<(String, u32)>,
    /// Files importing it — the tiebreak: of two equal matches, the one the
    /// code leans on is the one to read.
    pub users: usize,
}

const MAX_REEXPORT_DEPTH: usize = 5;

/// Cached extractions kept beyond the files mapped now — enough for a few
/// branches' worth of differing files.
const BLOB_CACHE_SPARE: usize = 20_000;

/// One file a search reached: the words it answers (by index into the
/// search's terms) and the matching names it defines.
type WhereHit = (HashSet<usize>, Vec<(String, u32)>);

/// How many symbols and files each search word may bring in before ranking.
const CANDIDATES_PER_TERM: usize = 400;

/// Words a question carries that name nothing in code.
const STOP_WORDS: [&str; 24] = [
    "how", "does", "do", "the", "a", "an", "is", "are", "what", "where", "who", "which", "and",
    "or", "of", "to", "in", "for", "with", "when", "why", "work", "works", "it",
];

/// The words of a search, lower-cased, without common words or ones too
/// short to mean anything.
fn search_terms(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text.split(|c: char| !c.is_alphanumeric() && c != '_') {
        let w = word.to_lowercase();
        if w.len() >= 3 && !STOP_WORDS.contains(&w.as_str()) && !out.contains(&w) {
            out.push(w);
        }
    }
    out
}
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

    /// Every file path in a registered root's repository at its revision.
    fn root_tree(&self, root_id: &str) -> Result<HashSet<String>, String> {
        let Some(root) = self.config.code.iter().find(|r| r.id == root_id) else {
            return Err(format!(
                "`{root_id}` is not a registered code root — add it under `code:`"
            ));
        };
        let repo = self.repo_for(root.repo.as_deref())?;
        let files = repo
            .get()
            .ls_tree_at(root.rev(), ".")
            .map_err(|e| e.to_string())?;
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
        match repo
            .get()
            .commits_touching_between(pin, root.rev(), &example.glob)
        {
            Ok(0) => "holds".to_owned(),
            Ok(n) => format!("stale:{n}"),
            Err(_) => "unconfirmed".to_owned(),
        }
    }

    /// Every path literal in the code, matched against the registered
    /// specs' operations. Read from the index only (I2); nothing is stored
    /// (I5). A heuristic over literals, and it says so: a path assembled at
    /// runtime from variables is not seen.
    pub fn code_api(&self) -> Result<ApiReport, DitError> {
        // The operations, and where each is proven.
        let mut ops: Vec<(ApiOperation, Vec<dit_model::ApiSegment>)> = Vec::new();
        for spec in &self.config.specs {
            for op in self.index.morse_operations(&spec.id)? {
                let segments = dit_model::api_segments(&op.path);
                ops.push((
                    ApiOperation {
                        spec: spec.id.clone(),
                        operation_id: op.operation_id,
                        method: op.method,
                        path: op.path,
                        proven: Vec::new(),
                    },
                    segments,
                ));
            }
        }
        let proven_envs: HashMap<String, Vec<String>> = self
            .morse_report()?
            .scenarios
            .into_iter()
            .map(|s| {
                let envs = s
                    .proofs
                    .iter()
                    .filter(|p| p.holds())
                    .map(|p| p.env.clone())
                    .collect();
                (s.scenario, envs)
            })
            .collect();
        for stored in self.index.morse_scenarios()? {
            let Some(envs) = proven_envs.get(&stored.scenario).filter(|e| !e.is_empty()) else {
                continue;
            };
            let Ok(scenario) = dit_parse::parse_morse_scenario(&stored.body) else {
                continue;
            };
            for step in &scenario.steps {
                let hit: Vec<usize> = match &step.operation {
                    dit_model::StepTarget::Operation(r) => ops
                        .iter()
                        .enumerate()
                        .filter(|(_, (o, _))| o.spec == r.spec && o.operation_id == r.operation)
                        .map(|(i, _)| i)
                        .collect(),
                    dit_model::StepTarget::Inline(id) => {
                        let Some(req) = scenario.requests.iter().find(|q| &q.id == id) else {
                            continue;
                        };
                        let segs = dit_model::api_segments(&req.path);
                        ops.iter()
                            .enumerate()
                            .filter(|(_, (o, s))| {
                                o.method.eq_ignore_ascii_case(&req.method)
                                    && dit_model::api_fit(&segs, s)
                                        == dit_model::ApiFit::Calls { guessed: 0 }
                            })
                            .map(|(i, _)| i)
                            .collect()
                    }
                };
                for i in hit {
                    for env in envs {
                        if !ops[i].0.proven.contains(env) {
                            ops[i].0.proven.push(env.clone());
                        }
                    }
                }
            }
        }
        let mut roots: Vec<String> = ops
            .iter()
            .filter_map(|(_, s)| match s.first() {
                Some(dit_model::ApiSegment::Lit(l)) => Some(l.clone()),
                _ => None,
            })
            .collect();
        roots.sort();
        roots.dedup();

        // Constants, and the names each file takes from another.
        let mut consts: HashMap<(String, String), HashMap<String, String>> = HashMap::new();
        for (root, path, name, value) in self.index.code_consts()? {
            consts.entry((root, path)).or_default().insert(name, value);
        }
        let mut imported: HashMap<(String, String), HashMap<String, String>> = HashMap::new();
        for imp in self.index.code_named_imports()? {
            let into = imported.entry((imp.root, imp.path)).or_default();
            for n in imp.names {
                into.entry(n).or_insert_with(|| imp.target.clone());
            }
        }
        let lookup = Lookup {
            dit: self,
            consts: &consts,
            imported: &imported,
        };

        let mut calls = Vec::new();
        let mut generic = 0;
        for (root, path, written, line) in self.index.code_strings()? {
            let resolved = lookup.expand(&root, &path, &written, 0);
            let mut segs = dit_model::api_segments(&resolved);
            // A leading value nobody can read — a base URL from the
            // environment — is where the host goes, not part of the path.
            if segs.first() == Some(&dit_model::ApiSegment::Any) && resolved.starts_with("${") {
                segs.remove(0);
            }
            let Some(dit_model::ApiSegment::Lit(first)) = segs.first() else {
                continue;
            };
            if !roots.contains(first) {
                continue;
            }
            let mut prefix = false;
            let mut fits: Vec<(usize, &ApiOperation)> = Vec::new();
            for (op, spec) in &ops {
                match dit_model::api_fit(&segs, spec) {
                    dit_model::ApiFit::Calls { guessed } => fits.push((guessed, op)),
                    dit_model::ApiFit::Prefix => prefix = true,
                    dit_model::ApiFit::None => {}
                }
            }
            // Keep only the closest fits: `items/${id}` calls `items/{id}`,
            // not `items/bulk`.
            let closest = fits.iter().map(|(g, _)| *g).min();
            if closest.is_some_and(|g| g > MAX_GUESSED) {
                generic += 1;
                continue;
            }
            let operations: Vec<ApiOperation> = fits
                .into_iter()
                .filter(|(g, _)| Some(*g) == closest)
                .map(|(_, op)| op.clone())
                .collect();
            if operations.is_empty() && prefix {
                continue;
            }
            calls.push(ApiCall {
                root,
                path,
                line,
                written,
                resolved,
                operations,
            });
        }
        Ok(ApiReport {
            calls,
            roots,
            operations: ops.len(),
            generic,
        })
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
            let head = repo.get().resolve(root.rev()).map_err(|e| {
                DitError::Refuse(format!(
                    "`{root_id}` has no commit at `{}`: {e}",
                    root.rev()
                ))
            })?;
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
        self.index.clear_blob_cache()?;
        self.index.set_watermark(EXTRACTOR_KEY, "")?;
        Ok(())
    }

    fn refresh_root(&mut self, root: &CodeRoot, report: &mut CodeReport) -> Result<(), String> {
        let repo_ref = self.repo_for(root.repo.as_deref())?;
        let repo = repo_ref.get();
        let listed = repo
            .ls_tree_at(root.rev(), ".")
            .map_err(|e| e.to_string())?;
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
        let aliases = tsconfig_aliases(
            repo.show_text(&format!("{}:tsconfig.json", root.rev()))
                .as_deref(),
        );

        // Read what changed in one batch, then parse it across threads —
        // outside the index borrow.
        let known = self.index.code_blobs(&root.id).map_err(|e| e.to_string())?;
        let changed: Vec<(String, String, CodeLang)> = covered
            .iter()
            .filter(|(path, blob)| known.get(path) != Some(blob))
            .filter_map(|(path, blob)| CodeLang::of(path).map(|l| (path.clone(), blob.clone(), l)))
            .collect();
        // A blob read before — on another branch, at another path — comes
        // from the cache; only blobs never seen are read and parsed.
        let keys: Vec<(String, String)> = changed
            .iter()
            .map(|(_, b, l)| (b.clone(), l.as_str().to_owned()))
            .collect();
        let cached = self.index.cached_facts(&keys).map_err(|e| e.to_string())?;
        let mut reused: Vec<(String, String, CodeLang, FileFacts)> = Vec::new();
        let mut missing: Vec<(String, String, CodeLang)> = Vec::new();
        for (path, blob, lang) in changed {
            let hit = cached
                .get(&(blob.clone(), lang.as_str().to_owned()))
                .and_then(|json| serde_json::from_str::<FileFacts>(json).ok());
            match hit {
                Some(facts) => reused.push((path, blob, lang, facts)),
                None => missing.push((path, blob, lang)),
            }
        }
        let shas: Vec<String> = missing.iter().map(|(_, b, _)| b.clone()).collect();
        let texts = repo.read_blobs(&shas).map_err(|e| e.to_string())?;
        drop(repo_ref);
        let work: Vec<(String, String, CodeLang, String)> = missing
            .into_iter()
            .zip(texts)
            .filter_map(|((path, blob, lang), text)| text.map(|t| (path, blob, lang, t)))
            .collect();
        let fresh = parse_all(work);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let stored: Vec<(String, String, String)> = fresh
            .iter()
            .filter_map(|(_, blob, lang, facts)| {
                serde_json::to_string(facts)
                    .ok()
                    .map(|j| (blob.clone(), lang.as_str().to_owned(), j))
            })
            .collect();
        let touched: Vec<(String, String)> = reused
            .iter()
            .map(|(_, b, l, _)| (b.clone(), l.as_str().to_owned()))
            .collect();
        self.index
            .store_facts(&stored, &touched, now)
            .map_err(|e| e.to_string())?;
        report.parsed += fresh.len();
        report.reused += reused.len();
        let parsed: Vec<(String, String, CodeLang, FileFacts)> =
            fresh.into_iter().chain(reused).collect();

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
                    root.is_generated(path) || facts.generated,
                    facts,
                )
                .map_err(|e| e.to_string())?;
        }
        report.files += covered.len();
        self.index
            .prune_blob_cache(BLOB_CACHE_SPARE.max(covered.len() * 2))
            .map_err(|e| e.to_string())?;

        // Kotlin imports name declarations: index them by qualified name.
        let mut kotlin = dit_code::KotlinIndex::default();
        for (path, package, name) in self
            .index
            .kotlin_decls(&root.id)
            .map_err(|e| e.to_string())?
        {
            kotlin.add_decl(&package, &name, &path);
        }
        let references = self
            .index
            .kotlin_references(&root.id)
            .map_err(|e| e.to_string())?;
        for file in &references {
            if let Some(p) = &file.package {
                kotlin.add_package(p);
            }
        }

        // Resolve every import against the files that exist now.
        let index = dit_code::RootIndex {
            files: &code_files,
            aliases: &aliases,
            kotlin: &kotlin,
        };
        let imports = self
            .index
            .code_imports_of_root(&root.id)
            .map_err(|e| e.to_string())?;
        let mut targets = Vec::new();
        for imp in &imports {
            let lang = CodeLang::parse(&imp.lang).unwrap_or(CodeLang::Tsx);
            let (target, external) =
                match dit_code::resolve(lang, &imp.path, &imp.specifier, &index) {
                    dit_code::Resolved::File(f) => (Some(f), false),
                    dit_code::Resolved::External => (None, true),
                    dit_code::Resolved::Package => (None, false),
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
        let implied = implied_imports(&references, &kotlin.decls);
        self.index
            .replace_implied_imports(&root.id, &implied)
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

    /// The files to read for a question: every word of `text` (common words
    /// dropped) is looked up in file paths and defined names, and files are
    /// ranked by how many words they answer, then by how much of the code
    /// imports them. Generated files and tests sink, never vanish.
    pub fn code_where(&self, text: &str, limit: usize) -> Result<Vec<CodeMatch>, DitError> {
        let terms = search_terms(text);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        // (root, path) → (terms matched, symbols)
        let mut hits: HashMap<(String, String), WhereHit> = HashMap::new();
        for (t, term) in terms.iter().enumerate() {
            for s in self.index.code_symbols_like(term, CANDIDATES_PER_TERM)? {
                let hit = hits.entry((s.root, s.path)).or_default();
                hit.0.insert(t);
                if !hit.1.iter().any(|(n, _)| n == &s.name) {
                    hit.1.push((s.name, s.line));
                }
            }
            for (root, path) in self.index.code_files_like(term, CANDIDATES_PER_TERM)? {
                hits.entry((root, path)).or_default().0.insert(t);
            }
        }
        let mut out: Vec<CodeMatch> = Vec::with_capacity(hits.len());
        for ((root, path), (matched, mut symbols)) in hits {
            // A symbol naming more of the words comes first.
            symbols.sort_by_key(|(n, _)| {
                let lower = n.to_lowercase();
                std::cmp::Reverse(terms.iter().filter(|t| lower.contains(t.as_str())).count())
            });
            let generated = self
                .index
                .code_file_generated(&root, &path)?
                .unwrap_or(false);
            let users = self.index.code_importers(&root, &path)?.len();
            out.push(CodeMatch {
                root,
                path,
                terms: matched.len(),
                generated,
                symbols,
                users,
            });
        }
        out.sort_by(|a, b| {
            let test =
                |p: &str| p.contains(".test.") || p.contains("__tests__") || p.contains("/tests/");
            b.terms
                .cmp(&a.terms)
                .then(a.generated.cmp(&b.generated))
                .then(test(&a.path).cmp(&test(&b.path)))
                .then(b.users.cmp(&a.users))
                .then(a.path.cmp(&b.path))
        });
        out.truncate(limit);
        Ok(out)
    }
}

/// Puts constants back into a literal: the file's own, a name it imports,
/// or `Obj.NAME` where `Obj` is declared in the same root.
struct Lookup<'a> {
    dit: &'a Dit,
    consts: &'a HashMap<(String, String), HashMap<String, String>>,
    imported: &'a HashMap<(String, String), HashMap<String, String>>,
}

impl Lookup<'_> {
    fn expand(&self, root: &str, path: &str, text: &str, depth: usize) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let Some(end) = after.find('}') else {
                out.push_str(&rest[start..]);
                return out;
            };
            let name = &after[..end];
            match (depth < MAX_CONST_DEPTH)
                .then(|| self.value(root, path, name))
                .flatten()
            {
                Some((at, value)) => out.push_str(&self.expand(root, &at, &value, depth + 1)),
                None => {
                    out.push_str("${");
                    out.push_str(name);
                    out.push('}');
                }
            }
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        out
    }

    /// The value behind a name, and the file it lives in (a constant built
    /// from others resolves them where it is defined).
    fn value(&self, root: &str, path: &str, name: &str) -> Option<(String, String)> {
        if name.is_empty() {
            return None;
        }
        let key = (root.to_owned(), path.to_owned());
        if let Some(v) = self.consts.get(&key).and_then(|c| c.get(name)) {
            return Some((path.to_owned(), v.clone()));
        }
        if let Some(target) = self.imported.get(&key).and_then(|i| i.get(name)) {
            let at = (root.to_owned(), target.clone());
            if let Some(v) = self.consts.get(&at).and_then(|c| c.get(name)) {
                return Some((target.clone(), v.clone()));
            }
        }
        // `Obj.NAME`: the constant inside the object or class declaring Obj.
        let (owner, member) = name.rsplit_once('.')?;
        let owner = owner.rsplit('.').next().unwrap_or(owner);
        let files = self.dit.index.code_symbols_named(owner).ok()?;
        files.into_iter().filter(|s| s.root == root).find_map(|s| {
            let at = (s.root.clone(), s.path.clone());
            self.consts
                .get(&at)
                .and_then(|c| c.get(member))
                .map(|v| (s.path.clone(), v.clone()))
        })
    }
}

/// The git hooks that move HEAD — after each, the map has files to read.
const HOOKS: [&str; 4] = ["post-commit", "post-merge", "post-checkout", "post-rewrite"];
const HOOK_BEGIN: &str = "# >>> dit code map";
const HOOK_END: &str = "# <<< dit code map";

fn hook_block() -> String {
    format!(
        "{HOOK_BEGIN}\n\
         # Refresh DIT's code map in the background once git has moved HEAD, so the\n\
         # next `dit code` question does not wait for it. Installed by\n\
         # `dit code hook install`; `dit code hook uninstall` removes this block.\n\
         if command -v dit >/dev/null 2>&1; then (dit code refresh >/dev/null 2>&1 &); fi\n\
         {HOOK_END}\n"
    )
}

/// Opt in to refreshing the code map in the background whenever git moves
/// HEAD. Each hook gets one marked block right after its shebang — an
/// existing hook keeps everything it had, and an `exit` further down cannot
/// skip the block. Returns the hooks written; refuses when git's hooks live
/// in tracked files (`core.hooksPath` pointing into the repository, as a hook
/// manager sets it), because installing would change what the team commits.
pub fn install_code_hooks(path: &std::path::Path) -> Result<Vec<String>, DitError> {
    let repo = dit_vcs::Repo::open(path)?;
    let dir = repo.hooks_dir()?;
    refuse_tracked_hooks(&repo, &dir)?;
    let mut written = Vec::new();
    for hook in HOOKS {
        let file = dir.join(hook);
        let current = std::fs::read_to_string(&file).unwrap_or_default();
        if current.contains(HOOK_BEGIN) {
            continue;
        }
        let updated = match current.split_once('\n') {
            Some((first, rest)) if first.starts_with("#!") => {
                format!("{first}\n{}{rest}", hook_block())
            }
            _ if current.trim().is_empty() => format!("#!/bin/sh\n{}", hook_block()),
            _ => format!("#!/bin/sh\n{}{current}", hook_block()),
        };
        dit_store::atomic::write_executable(&file, &updated)?;
        written.push(hook.to_owned());
    }
    Ok(written)
}

/// Take the block back out; a hook left with nothing but its shebang goes.
pub fn uninstall_code_hooks(path: &std::path::Path) -> Result<Vec<String>, DitError> {
    let repo = dit_vcs::Repo::open(path)?;
    let dir = repo.hooks_dir()?;
    let mut removed = Vec::new();
    for hook in HOOKS {
        let file = dir.join(hook);
        let Ok(current) = std::fs::read_to_string(&file) else {
            continue;
        };
        let (Some(start), Some(end)) = (current.find(HOOK_BEGIN), current.find(HOOK_END)) else {
            continue;
        };
        let end = end + HOOK_END.len();
        let end = if current[end..].starts_with('\n') {
            end + 1
        } else {
            end
        };
        let rest = format!("{}{}", &current[..start], &current[end..]);
        let empty = rest
            .lines()
            .all(|l| l.trim().is_empty() || l.starts_with("#!"));
        if empty {
            dit_store::atomic::remove_file(&file)?;
        } else {
            dit_store::atomic::write_executable(&file, &rest)?;
        }
        removed.push(hook.to_owned());
    }
    Ok(removed)
}

fn refuse_tracked_hooks(repo: &dit_vcs::Repo, dir: &std::path::Path) -> Result<(), DitError> {
    let Ok(inside) = dir.strip_prefix(repo.root()) else {
        return Ok(());
    };
    let rel = inside.to_string_lossy().replace('\\', "/");
    if rel.starts_with(".git/") || rel == ".git" {
        return Ok(());
    }
    let tracked =
        HOOKS.iter().any(|h| repo.is_tracked(&format!("{rel}/{h}"))) || repo.is_tracked(&rel);
    if tracked {
        return Err(DitError::Refuse(format!(
            "git runs hooks from `{rel}/`, which is committed (a hook manager set core.hooksPath) — \
             add `dit code refresh` to that manager's post-merge/post-checkout hooks instead, so \
             the change goes through review like any other"
        )));
    }
    Ok(())
}

/// The edges Kotlin does not write down: a file using a declaration of its
/// own package, or of a package it imports with `*`, depends on the file
/// declaring it. Read from the names it calls and extends — never from a
/// guess about a name nobody used.
fn implied_imports(
    references: &[KotlinReferences],
    decls: &HashMap<String, String>,
) -> Vec<(String, String, Vec<String>, u32, String)> {
    let mut out = Vec::new();
    for KotlinReferences {
        path,
        package,
        names,
        wildcards,
    } in references
    {
        // target → (specifier, names, first line)
        let mut edges: BTreeMap<String, (String, Vec<String>, u32)> = BTreeMap::new();
        let scopes = package
            .iter()
            .map(|p| (p.as_str(), format!("{p}.(package)")))
            .chain(wildcards.iter().map(|w| (w.as_str(), format!("{w}.*"))));
        for (scope, specifier) in scopes {
            for (name, line) in names {
                let Some(target) = decls.get(&format!("{scope}.{name}")) else {
                    continue;
                };
                if target == path {
                    continue;
                }
                let edge = edges
                    .entry(target.clone())
                    .or_insert_with(|| (specifier.clone(), Vec::new(), *line));
                if !edge.1.contains(name) {
                    edge.1.push(name.clone());
                }
                if *line > 0 && (edge.2 == 0 || *line < edge.2) {
                    edge.2 = *line;
                }
            }
        }
        for (target, (specifier, names, line)) in edges {
            out.push((path.clone(), specifier, names, line, target));
        }
    }
    out
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
