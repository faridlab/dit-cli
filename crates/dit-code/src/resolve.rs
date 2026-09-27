//! Import resolution: a specifier as written, to the file it names in the
//! same code root — or `External` for a package, or `Unresolved` when the
//! extractor cannot tell. Never a guess.

use std::collections::{HashMap, HashSet};

use dit_model::CodeLang;

/// Where an import leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    File(String),
    /// A package or crate outside this code root (`react`, `std`).
    External,
    /// Looks local but names no file here.
    Unresolved,
    /// A Kotlin wildcard import of a package in this root: it names every
    /// file of the package, and which of them it uses is read from the
    /// importer's calls, not from the import.
    Package,
}

/// What Kotlin resolution needs: imports name declarations, not files.
#[derive(Debug, Default)]
pub struct KotlinIndex {
    /// Fully qualified top-level declaration → the file declaring it.
    pub decls: HashMap<String, String>,
    /// Every package declared in the root.
    packages: HashSet<String>,
    /// The first two segments of each package — the reverse-domain prefix
    /// a project owns, which tells an unresolved import from a library.
    prefixes: HashSet<String>,
}

impl KotlinIndex {
    pub fn add_package(&mut self, package: &str) {
        if self.packages.insert(package.to_owned()) {
            self.prefixes.insert(prefix(package));
        }
    }

    pub fn add_decl(&mut self, package: &str, name: &str, path: &str) {
        self.add_package(package);
        self.decls
            .entry(format!("{package}.{name}"))
            .or_insert_with(|| path.to_owned());
    }

    fn is_internal(&self, qualified: &str) -> bool {
        self.prefixes.contains(&prefix(qualified))
    }
}

fn prefix(qualified: &str) -> String {
    qualified
        .splitn(3, '.')
        .take(2)
        .collect::<Vec<_>>()
        .join(".")
}

/// The files of one code root, and its TypeScript path aliases.
#[derive(Debug)]
pub struct RootIndex<'a> {
    pub files: &'a HashSet<String>,
    /// `(pattern, target)` from `tsconfig.json` `paths`, e.g. `("@/*", "src/*")`.
    pub aliases: &'a [(String, String)],
    pub kotlin: &'a KotlinIndex,
}

/// Resolve one specifier written in `from`.
pub fn resolve(lang: CodeLang, from: &str, specifier: &str, root: &RootIndex<'_>) -> Resolved {
    match lang {
        CodeLang::TypeScript | CodeLang::Tsx => resolve_ts(from, specifier, root),
        CodeLang::Rust => resolve_rust(from, specifier, root),
        CodeLang::Kotlin => resolve_kotlin(specifier, root.kotlin),
    }
}

/// `a.b.C` → the file declaring `C` in package `a.b`; `a.b.C.Inner` or
/// `a.b.C.member` → the file declaring `C`; `a.b.*` → the package.
fn resolve_kotlin(spec: &str, k: &KotlinIndex) -> Resolved {
    if let Some(pkg) = spec.strip_suffix(".*") {
        if k.packages.contains(pkg) {
            return Resolved::Package;
        }
        if let Some(file) = k.decls.get(pkg) {
            return Resolved::File(file.clone());
        }
    } else {
        let mut candidate = spec;
        loop {
            if let Some(file) = k.decls.get(candidate) {
                return Resolved::File(file.clone());
            }
            match candidate.rsplit_once('.') {
                Some((parent, _)) => candidate = parent,
                None => break,
            }
        }
    }
    if k.is_internal(spec) {
        Resolved::Unresolved
    } else {
        Resolved::External
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// Join and normalise `a/b/../c` → `a/c`; `None` when it climbs out of the root.
fn normalise(base: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

const TS_EXTS: [&str; 7] = [".ts", ".tsx", ".d.ts", ".js", ".jsx", ".mts", ".mjs"];

fn ts_candidates(stem: &str, files: &HashSet<String>) -> Option<String> {
    if files.contains(stem) {
        return Some(stem.to_owned());
    }
    // `./client.js` written for a `client.ts` source (ESM-style specifiers).
    for js in [".js", ".jsx", ".mjs"] {
        if let Some(base) = stem.strip_suffix(js) {
            for ext in [".ts", ".tsx", ".mts"] {
                let c = format!("{base}{ext}");
                if files.contains(&c) {
                    return Some(c);
                }
            }
        }
    }
    for ext in TS_EXTS {
        let c = format!("{stem}{ext}");
        if files.contains(&c) {
            return Some(c);
        }
    }
    for ext in TS_EXTS {
        let c = format!("{stem}/index{ext}");
        if files.contains(&c) {
            return Some(c);
        }
    }
    None
}

fn resolve_ts(from: &str, spec: &str, root: &RootIndex<'_>) -> Resolved {
    if spec.starts_with('.') {
        return normalise(parent(from), spec)
            .and_then(|stem| ts_candidates(&stem, root.files))
            .map_or(Resolved::Unresolved, Resolved::File);
    }
    for (pattern, target) in root.aliases {
        let hit = match (pattern.strip_suffix('*'), target.strip_suffix('*')) {
            (Some(pre), Some(tpre)) => spec.strip_prefix(pre).map(|rest| format!("{tpre}{rest}")),
            _ if spec == pattern => Some(target.clone()),
            _ => None,
        };
        if let Some(stem) = hit {
            return normalise("", &stem)
                .and_then(|stem| ts_candidates(&stem, root.files))
                .map_or(Resolved::Unresolved, Resolved::File);
        }
    }
    Resolved::External
}

/// The directory a Rust file's child modules live in: `lib.rs`, `main.rs`
/// and `mod.rs` own their directory; `x.rs` owns `x/`.
fn module_dir(file: &str) -> String {
    let (dir, name) = file.rsplit_once('/').unwrap_or(("", file));
    match name {
        "lib.rs" | "main.rs" | "mod.rs" => dir.to_owned(),
        other => {
            let stem = other.strip_suffix(".rs").unwrap_or(other);
            if dir.is_empty() {
                stem.to_owned()
            } else {
                format!("{dir}/{stem}")
            }
        }
    }
}

/// The file that defines the module a directory stands for.
fn module_file(dir: &str, files: &HashSet<String>) -> Option<String> {
    for c in [
        format!("{dir}/lib.rs"),
        format!("{dir}/main.rs"),
        format!("{dir}/mod.rs"),
    ] {
        if files.contains(&c) {
            return Some(c);
        }
    }
    let c = format!("{dir}.rs");
    files.contains(&c).then_some(c)
}

/// `crates/dit-core/src/…` → `crates/dit-core/src`: the nearest `src` above
/// holding a crate root.
fn crate_src(from: &str, files: &HashSet<String>) -> Option<String> {
    let mut dir = parent(from);
    loop {
        if dir.ends_with("src") || dir == "src" {
            let lib = format!("{dir}/lib.rs");
            let main = format!("{dir}/main.rs");
            if files.contains(&lib) || files.contains(&main) {
                return Some(dir.to_owned());
            }
        }
        if dir.is_empty() {
            return None;
        }
        dir = parent(dir);
    }
}

/// The `src` of a sibling crate named `ident` (`dit_model` → `…/dit-model/src`).
fn sibling_crate(ident: &str, files: &HashSet<String>) -> Option<String> {
    files
        .iter()
        .filter_map(|f| f.strip_suffix("/src/lib.rs"))
        .find(|dir| {
            dir.rsplit('/')
                .next()
                .is_some_and(|n| n.replace('-', "_") == ident)
        })
        .map(|dir| format!("{dir}/src"))
}

fn resolve_rust(from: &str, spec: &str, root: &RootIndex<'_>) -> Resolved {
    let files = root.files;
    let mut segs: Vec<&str> = spec.split("::").filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return Resolved::Unresolved;
    }
    // The directory the remaining segments are resolved under, and the file
    // that module itself is defined in.
    let (mut dir, mut file) = match segs[0] {
        "crate" => {
            let Some(src) = crate_src(from, files) else {
                return Resolved::Unresolved;
            };
            let f = module_file(&src, files);
            (src, f)
        }
        "self" => (module_dir(from), Some(from.to_owned())),
        "super" => {
            let here = module_dir(from);
            let up = parent(&here).to_owned();
            let f = module_file(&up, files);
            (up, f)
        }
        ident => match sibling_crate(ident, files) {
            Some(src) => {
                let f = module_file(&src, files);
                (src, f)
            }
            None => return Resolved::External,
        },
    };
    segs.remove(0);
    for seg in segs {
        if seg == "super" {
            dir = parent(&dir).to_owned();
            file = module_file(&dir, files);
            continue;
        }
        let next = format!("{dir}/{seg}");
        match module_file(&next, files) {
            Some(f) => {
                dir = next;
                file = Some(f);
            }
            // Not a module: an item inside the module reached so far.
            None => break,
        }
    }
    file.map_or(Resolved::Unresolved, Resolved::File)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn set(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|p| (*p).to_owned()).collect()
    }

    #[test]
    fn typescript_relative_alias_index_and_packages() {
        let files = set(&[
            "src/crud/hooks.ts",
            "src/crud/RecordSurface.tsx",
            "src/desks/people/views.tsx",
            "src/desks/people/verbs/index.ts",
            "src/lib/api/client.ts",
        ]);
        let aliases = vec![("@/*".to_owned(), "src/*".to_owned())];
        let root = RootIndex {
            files: &files,
            aliases: &aliases,
            kotlin: &KotlinIndex::default(),
        };
        let from = "src/desks/people/PayrollRunsPage.tsx";
        let r = |spec: &str| resolve(CodeLang::Tsx, from, spec, &root);
        assert_eq!(
            r("./views"),
            Resolved::File("src/desks/people/views.tsx".into())
        );
        assert_eq!(
            r("./verbs"),
            Resolved::File("src/desks/people/verbs/index.ts".into())
        );
        assert_eq!(
            r("../../crud/hooks"),
            Resolved::File("src/crud/hooks.ts".into())
        );
        assert_eq!(
            r("@/crud/RecordSurface"),
            Resolved::File("src/crud/RecordSurface.tsx".into())
        );
        assert_eq!(
            r("@/lib/api/client.js"),
            Resolved::File("src/lib/api/client.ts".into())
        );
        assert_eq!(r("react"), Resolved::External);
        assert_eq!(r("@mantine/core"), Resolved::External);
        assert_eq!(r("./missing"), Resolved::Unresolved);
        assert_eq!(r("@/nowhere"), Resolved::Unresolved);
    }

    #[test]
    fn rust_crate_self_super_modules_and_sibling_crates() {
        let files = set(&[
            "crates/dit-core/src/lib.rs",
            "crates/dit-core/src/agent.rs",
            "crates/dit-core/src/morse.rs",
            "crates/dit-core/src/flow/mod.rs",
            "crates/dit-core/src/flow/board.rs",
            "crates/dit-model/src/lib.rs",
            "crates/dit-model/src/code.rs",
        ]);
        let root = RootIndex {
            files: &files,
            aliases: &[],
            kotlin: &KotlinIndex::default(),
        };
        let lib = "crates/dit-core/src/lib.rs";
        let r = |from: &str, spec: &str| resolve(CodeLang::Rust, from, spec, &root);
        assert_eq!(
            r(lib, "self::agent"),
            Resolved::File("crates/dit-core/src/agent.rs".into())
        );
        assert_eq!(
            r(lib, "self::flow"),
            Resolved::File("crates/dit-core/src/flow/mod.rs".into())
        );
        assert_eq!(
            r(lib, "crate::morse"),
            Resolved::File("crates/dit-core/src/morse.rs".into())
        );
        // An item path resolves to the module that holds the item.
        assert_eq!(
            r(lib, "crate::flow::board::Board"),
            Resolved::File("crates/dit-core/src/flow/board.rs".into())
        );
        assert_eq!(
            r("crates/dit-core/src/flow/board.rs", "super"),
            Resolved::File("crates/dit-core/src/flow/mod.rs".into())
        );
        assert_eq!(
            r("crates/dit-core/src/flow/mod.rs", "self::board"),
            Resolved::File("crates/dit-core/src/flow/board.rs".into())
        );
        assert_eq!(
            r(lib, "dit_model"),
            Resolved::File("crates/dit-model/src/lib.rs".into())
        );
        assert_eq!(
            r(lib, "dit_model::code"),
            Resolved::File("crates/dit-model/src/code.rs".into())
        );
        assert_eq!(r(lib, "std::path"), Resolved::External);
        assert_eq!(
            r(lib, "crate::nowhere::deep"),
            Resolved::File("crates/dit-core/src/lib.rs".into())
        );
    }

    #[test]
    fn kotlin_imports_resolve_by_declaration() {
        let files = HashSet::new();
        let mut k = KotlinIndex::default();
        k.add_package("com.acme.ui");
        k.add_decl("com.acme.data", "Repo", "src/data/Repo.kt");
        let root = RootIndex {
            files: &files,
            aliases: &[],
            kotlin: &k,
        };
        let r = |s: &str| resolve(CodeLang::Kotlin, "src/ui/A.kt", s, &root);
        assert_eq!(
            r("com.acme.data.Repo"),
            Resolved::File("src/data/Repo.kt".into())
        );
        assert_eq!(
            r("com.acme.data.Repo.Companion"),
            Resolved::File("src/data/Repo.kt".into())
        );
        assert_eq!(r("com.acme.data.*"), Resolved::Package);
        assert_eq!(r("com.acme.data.Gone"), Resolved::Unresolved);
        assert_eq!(r("kotlinx.coroutines.launch"), Resolved::External);
    }
}
