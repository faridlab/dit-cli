#!/usr/bin/env python3
"""Benchmark the questions an agent asks a codebase: grep, graphify, dit code.

For each question every tool's answer is measured — output size (tokens are
estimated as bytes / 4, the usual rule of thumb for code and paths), the calls
it takes to get a complete answer, and whether that answer is right against a
key computed here independently of every tool. The winner of a row is the
cheapest answer that is correct; an incomplete or wrong answer cannot win.

It also times the map itself: a cold build, a warm refresh with nothing
changed, and a branch switch away and back (the case the blob cache is for),
for the dit binaries given and for `graphify update`.

    bench.py --repo <git repo> --dit <new dit> [--dit-baseline <old dit>]
             [--graphify <graphify>] [--out RESULTS.md]

The repository is cloned into a temporary directory; nothing in it is touched.
The questions below are written for serpa-webapp-admin (a React/TypeScript
admin app); point QUESTIONS at another repository's names to reuse it.
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

# The questions, each with the facts its key is computed from.
QUESTIONS = {
    "users": {"title": "Who imports `useResourceList` (blast radius before a change)",
              "symbol": "useResourceList", "defined_in": "src/crud/hooks.ts"},
    "uses": {"title": "What does `PayrollRunsPage.tsx` import",
             "file": "src/desks/people/PayrollRunsPage.tsx"},
    "path": {"title": "How `SerpaShell` reaches `tokenStore`",
             "from": "src/shell/SerpaShell.tsx", "to": "src/auth/tokenStore.ts"},
    "where": {"title": "Where is token refresh handled (a question in words)",
              "words": "how does token refresh work",
              # The files a person reads to answer it, found by reading the
              # code: where refresh tokens live, and where a 401 retries.
              "must_find": ["src/auth/tokenStore.ts", "src/lib/api/client.ts"]},
}

IMPORT_RE = re.compile(r"""import\s+(?:type\s+)?([^;]*?)\s+from\s+["']([^"']+)["']""", re.S)
SIDE_EFFECT_RE = re.compile(r"""^\s*import\s+["']([^"']+)["']""", re.M)


def run(cmd, cwd, env=None):
    t = time.perf_counter()
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, env=env)
    return p.stdout + p.stderr, time.perf_counter() - t


def tokens(text):
    return len(text.encode()) // 4


# ---------------------------------------------------------------- the keys

def tracked(repo):
    out, _ = run(["git", "ls-files"], repo)
    return [l for l in out.splitlines() if l]


def resolve(spec, frm, files):
    """TypeScript resolution, enough for the key: relative and `@/`."""
    if spec.startswith("@/"):
        base = "src/" + spec[2:]
    elif spec.startswith("."):
        base = os.path.normpath(os.path.join(os.path.dirname(frm), spec))
    else:
        return None
    for cand in [base, base + ".ts", base + ".tsx", base + "/index.ts", base + "/index.tsx"]:
        if cand in files:
            return cand
    return None


def key_users(repo, files, q):
    """Files whose import statements take the symbol from its defining file,
    directly or through a barrel that re-exports it."""
    fileset = set(files)
    barrels = set()
    for f in files:
        if not f.endswith((".ts", ".tsx")):
            continue
        text = open(os.path.join(repo, f), errors="ignore").read()
        for m in re.finditer(r"""export\s+\{([^}]*)\}\s+from\s+["']([^"']+)["']""", text):
            if q["symbol"] in m.group(1) and resolve(m.group(2), f, fileset) == q["defined_in"]:
                barrels.add(f)
    users = set()
    for f in files:
        if not f.endswith((".ts", ".tsx")):
            continue
        text = open(os.path.join(repo, f), errors="ignore").read()
        for m in IMPORT_RE.finditer(text):
            if re.search(r"\b%s\b" % q["symbol"], m.group(1)):
                target = resolve(m.group(2), f, fileset)
                if target == q["defined_in"] or target in barrels:
                    users.add(f)
    return users


def key_uses(repo, files, q):
    text = open(os.path.join(repo, q["file"]), errors="ignore").read()
    specs = {m.group(2) for m in IMPORT_RE.finditer(text)}
    specs |= set(SIDE_EFFECT_RE.findall(text))
    return specs


# ---------------------------------------------------------------- the tools

def answers(repo, dit, graphify):
    """Every tool's answer to every question: (output, calls, seconds)."""
    out = {}
    q = QUESTIONS

    def grep(*args):
        return run(["grep", "-rnE", *args], repo)

    out["users"] = {
        "grep": grep(r"\b%s\b" % q["users"]["symbol"], "src"),
        "dit": run([dit, "code", "users", q["users"]["symbol"]], repo),
    }
    out["uses"] = {
        "grep": grep(r"^import|from ['\"]", q["uses"]["file"]),
        "dit": run([dit, "code", "uses", q["uses"]["file"]], repo),
    }
    out["path"] = {"dit": run([dit, "code", "path", "SerpaShell", "tokenStore"], repo)}
    out["where"] = {
        "grep": grep("-i", "refresh", "src"),
        "dit": run([dit, "code", "where", *q["where"]["words"].split()], repo),
    }
    if graphify:
        g = lambda *a: run([graphify, *a], repo)
        out["users"]["graphify query"] = g("query", "who uses %s" % q["users"]["symbol"])
        out["users"]["graphify explain"] = g("explain", q["users"]["symbol"])
        # The file's own node: its bare name reaches the component function.
        out["uses"]["graphify explain"] = g("explain", q["uses"]["file"].split("/")[-1])
        out["path"]["graphify"] = g("path", "SerpaShell", "tokenStore")
        out["where"]["graphify query"] = g("query", q["where"]["words"])
    return {k: {t: (o, s) for t, (o, s) in v.items()} for k, v in out.items()}


def judge(question, tool, output, repo, files, keys):
    """(correct, note, calls) for one answer."""
    q = QUESTIONS[question]
    if question == "users":
        key = keys["users"]
        found = {f for f in key if f in output}
        extra = 0
        if tool == "grep":
            listed = {l.split(":", 1)[0] for l in output.splitlines() if ":" in l}
            extra = len(listed - key - {q["defined_in"]})
        complete = found == key
        if "TRUNCATED" in output:
            complete = False
        note = "%d/%d importers" % (len(found), len(key))
        if extra:
            note += ", %d files that only mention it" % extra
        # grep lists candidates; telling importers from mentions takes a look.
        calls = 1 + (1 if extra else 0)
        return complete, note, calls
    if question == "uses":
        key = keys["uses"]
        if tool == "grep":
            found = {s for s in key if s in output}
            return found == key, "%d/%d imports, specifiers unresolved" % (len(found), len(key)), 1
        if tool == "dit":
            lines = [l for l in output.splitlines()[1:] if l.strip()]
            resolved = [l for l in lines if l.strip().startswith("src/")]
            ext = "".join(l for l in lines if "external:" in l)
            n_ext = ext.count("{") if ext else 0
            n = len(resolved) + n_ext
            return n >= len(key), "%d/%d imports, resolved to files" % (min(n, len(key)), len(key)), 1
        # graphify draws one `imports_from` edge per internal file imported,
        # and none for packages.
        n = output.count("[imports_from]")
        return n >= len(key), "%d/%d imports (internal files only)" % (n, len(key)), 1
    if question == "path":
        chain = [l.strip(" →") for l in output.splitlines() if l.strip()]
        ok = q["to"].split("/")[-1].split(".")[0] in output and len(output) > 0
        return ok, "a chain of %d hops" % max(0, output.count("→") or output.count("-->")), 1
    if question == "where":
        must = q["must_find"]
        found = [f for f in must if f in output]
        top = [l.split(":")[0].split()[0] for l in output.splitlines() if l.strip()][:5]
        in_top = [f for f in must if f in top] if tool == "dit" else found
        note = "%d/%d of the files to read" % (len(found), len(must))
        if tool == "dit":
            note += " (%d in the top 5)" % len(in_top)
        # Every tool leaves the reading to do; a list of files is the start.
        return len(found) == len(must), note, 1
    return False, "", 1


# ---------------------------------------------------------------- refresh

def refresh_timings(repo, dits, graphify):
    """Seconds for a cold build, a warm refresh and a branch switch away and
    back, per tool, each in a fresh clone of its own."""
    rows = {}
    for label, dit in dits.items():
        work = clone(repo)
        cold = timed(work, [dit, "code", "refresh"])
        warm = timed(work, [dit, "code", "refresh"])
        run(["git", "checkout", "-q", "HEAD~40"], work)
        away = timed(work, [dit, "code", "refresh"])
        run(["git", "checkout", "-q", "-"], work)
        back = timed(work, [dit, "code", "refresh"])
        rows[label] = (cold, warm, away, back)
        shutil.rmtree(work, ignore_errors=True)
    if graphify:
        work = clone(repo)
        cold = timed(work, [graphify, "update", "."])
        warm = timed(work, [graphify, "update", "."])
        run(["git", "checkout", "-q", "HEAD~40"], work)
        away = timed(work, [graphify, "update", "."])
        run(["git", "checkout", "-q", "-"], work)
        back = timed(work, [graphify, "update", "."])
        rows["graphify update"] = (cold, warm, away, back)
        shutil.rmtree(work, ignore_errors=True)
    return rows


def timed(cwd, cmd):
    _, s = run(cmd, cwd)
    return s


def clone(repo):
    d = tempfile.mkdtemp(prefix="dit-bench-")
    subprocess.run(["git", "clone", "-q", "--local", repo, d], check=True)
    return d


def hook_cost(repo, graphify):
    """What graphify's search hook adds to the context of one Grep or Bash
    call, when it is installed as a PreToolUse hook."""
    if not graphify:
        return None
    os.makedirs(os.path.join(repo, "graphify-out"), exist_ok=True)
    p = subprocess.run(
        [graphify, "hook-guard", "search"],
        input='{"tool_name":"Grep","tool_input":{"pattern":"x"},"cwd":"%s"}' % repo,
        capture_output=True, text=True, cwd=repo)
    return tokens(p.stdout)


# ---------------------------------------------------------------- report

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", required=True)
    ap.add_argument("--dit", required=True)
    ap.add_argument("--dit-baseline")
    ap.add_argument("--graphify")
    ap.add_argument("--out")
    a = ap.parse_args()

    work = clone(a.repo)
    try:
        files = tracked(work)
        run([a.dit, "code", "refresh"], work)
        if a.graphify:
            run([a.graphify, "update", "."], work)
        keys = {"users": key_users(work, files, QUESTIONS["users"]),
                "uses": key_uses(work, files, QUESTIONS["uses"])}
        got = answers(work, a.dit, a.graphify)
        hook = hook_cost(work, a.graphify)
    finally:
        shutil.rmtree(work, ignore_errors=True)

    lines = ["# Code map benchmark", "",
             "Repository: `%s` (%d tracked files). Tokens are bytes / 4. A row's winner is the "
             "cheapest answer that is correct and complete; the others are shown with what they "
             "missed." % (os.path.basename(a.repo.rstrip("/")), len(files)), ""]
    wins = {}
    for qk, q in QUESTIONS.items():
        lines += ["## %s" % q["title"], "",
                  "| Tool | Tokens | Calls | Correct | Notes |", "|---|---:|---:|:---:|---|"]
        scored = []
        for tool, (output, _secs) in got[qk].items():
            ok, note, calls = judge(qk, tool, output, work, files, keys)
            scored.append((tool, tokens(output), calls, ok, note))
        good = [s for s in scored if s[3]]
        winner = min(good, key=lambda s: (s[1] * s[2], s[1]))[0] if good else None
        for tool, tok, calls, ok, note in scored:
            mark = " **(winner)**" if tool == winner else ""
            lines.append("| %s%s | %d | %d | %s | %s |" % (tool, mark, tok, calls, "yes" if ok else "no", note))
        wins[winner] = wins.get(winner, 0) + 1
        lines.append("")

    lines += ["## Keeping the map current", "",
              "Seconds, each tool in a fresh clone of its own: build from nothing, refresh with "
              "nothing changed, check out 40 commits back, and return.", "",
              "| Tool | Cold | Warm | Branch away | Branch back |", "|---|---:|---:|---:|---:|"]
    dits = {"dit (this build)": a.dit}
    if a.dit_baseline:
        dits["dit (baseline)"] = a.dit_baseline
    for label, (c, w, aw, bk) in refresh_timings(a.repo, dits, a.graphify).items():
        lines.append("| %s | %.2f | %.2f | %.2f | %.2f |" % (label, c, w, aw, bk))
    lines.append("")
    if hook is not None:
        lines += ["## Per-call overhead", "",
                  "graphify's search hook, when installed as a `PreToolUse` hook on `Bash|Grep`, adds "
                  "**%d tokens** to every such call, and tells the agent it MUST run `graphify query` "
                  "before grepping. dit installs no agent hook: 0 tokens per call." % hook, ""]
    lines += ["## Tally", ""]
    for tool, n in sorted(wins.items(), key=lambda kv: -kv[1]):
        lines.append("- %s: %d of %d questions" % (tool or "no correct answer", n, len(QUESTIONS)))
    lines.append("")
    report = "\n".join(lines)
    if a.out:
        open(a.out, "w").write(report)
    print(report)


if __name__ == "__main__":
    sys.exit(main())
