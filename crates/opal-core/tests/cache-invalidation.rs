//! The invalidation matrix.
//!
//! A bug here does not crash — it serves a stale graph. So every case asserts
//! the *reason* for a miss, not merely that something missed, and the hit cases
//! assert the cached graph is identical to a freshly resolved one rather than
//! merely present.

mod support;

use std::collections::BTreeSet;

use opal_core::cas::gc::{self, GcOptions};
use opal_core::graph::{CacheStatus, MissReason, ResolverOptions, resolve_cached};
use support::{Project, cache, entry, reopen};

fn resolve(
    cache: &opal_core::graph::GraphCache,
    project: &Project,
    from: &str,
) -> (opal_core::graph::ModuleGraph, CacheStatus) {
    let resolved = resolve_cached(
        cache,
        &project.root(),
        &entry(from),
        &ResolverOptions::default(),
    )
    .expect("resolve");
    (resolved.graph, resolved.status)
}

fn app() -> Project {
    let project = Project::new();
    project
        .write("index.js", "import './direct.js';\n")
        .write("direct.js", "import './transitive.js';\n")
        .write("transitive.js", "export const value = 1;\n");
    project
}

#[test]
fn test_unchanged_input_hits_with_an_identical_graph() {
    let project = app();
    let (directory, cache) = cache();

    let (first, cold) = resolve(&cache, &project, "index.js");
    let (second, warm) = resolve(&cache, &project, "index.js");

    assert_eq!(cold, CacheStatus::Miss(MissReason::NoRecord));
    assert_eq!(warm, CacheStatus::Hit);
    assert_eq!(first.digest(), second.digest());
    assert_eq!(first.to_json(), second.to_json());
    drop(directory);
}

#[test]
fn test_changing_file_content_misses() {
    let project = app();
    let (_directory, cache) = cache();

    resolve(&cache, &project, "index.js");
    project.write("transitive.js", "export const value = 2;\n");

    let (_, status) = resolve(&cache, &project, "index.js");
    assert_eq!(
        status,
        CacheStatus::Miss(MissReason::InputChanged(entry("transitive.js")))
    );
}

#[test]
fn test_rewriting_identical_content_still_hits() {
    let project = app();
    let (_directory, cache) = cache();

    resolve(&cache, &project, "index.js");
    project.write("transitive.js", "export const value = 1;\n");

    let (_, status) = resolve(&cache, &project, "index.js");
    assert_eq!(status, CacheStatus::Hit);
}

#[test]
fn test_touching_mtime_without_changing_content_still_hits() {
    // The "never mtime" invariant (PRD §4.2). Guarded directly, because the
    // cheap way to speed up hashing is exactly the change that breaks it.
    let project = app();
    let (_directory, cache) = cache();

    resolve(&cache, &project, "index.js");
    project.touch_mtime("transitive.js");
    project.touch_mtime("index.js");

    let (_, status) = resolve(&cache, &project, "index.js");
    assert_eq!(status, CacheStatus::Hit);
}

#[test]
fn test_removing_a_file_misses() {
    let project = app();
    let (_directory, cache) = cache();

    resolve(&cache, &project, "index.js");
    project.remove("transitive.js");

    let (_, status) = resolve(&cache, &project, "index.js");
    assert_eq!(
        status,
        CacheStatus::Miss(MissReason::InputRemoved(entry("transitive.js")))
    );
}

#[test]
fn test_adding_a_file_that_shadows_a_resolution_misses() {
    // Nothing that was read has changed — only a path that was probed and found
    // absent now exists. Without negative dependencies this is a stale hit.
    let project = Project::new();
    project
        .write("index.js", "import './x';\n")
        .write("x/index.js", "export const x = 1;\n");
    let (_directory, cache) = cache();

    let (before, _) = resolve(&cache, &project, "index.js");
    project.write("x.js", "export const x = 2;\n");
    let (after, status) = resolve(&cache, &project, "index.js");

    assert_eq!(
        status,
        CacheStatus::Miss(MissReason::MissingPathAppeared(entry("x.js")))
    );
    assert_ne!(before.digest(), after.digest());
}

#[test]
fn test_adding_an_unrelated_file_still_hits() {
    let project = app();
    let (_directory, cache) = cache();

    resolve(&cache, &project, "index.js");
    project.write("docs/readme.md", "unrelated\n");

    let (_, status) = resolve(&cache, &project, "index.js");
    assert_eq!(status, CacheStatus::Hit);
}

#[test]
fn test_changing_resolver_options_misses_without_disturbing_the_old_entry() {
    let project = app();
    let (_directory, cache) = cache();

    resolve(&cache, &project, "index.js");

    let changed = ResolverOptions {
        follow_type_only_imports: true,
        ..ResolverOptions::default()
    };
    let other = resolve_cached(&cache, &project.root(), &entry("index.js"), &changed).unwrap();
    assert_eq!(other.status, CacheStatus::Miss(MissReason::NoRecord));

    let (_, status) = resolve(&cache, &project, "index.js");
    assert_eq!(status, CacheStatus::Hit);
}

#[test]
fn test_a_second_tool_hits_the_shared_cache() {
    // testing_strategy.md §3: the seam no single crate's unit tests cover.
    let project = app();
    let (directory, first_tool) = cache();

    let (from_first, cold) = resolve(&first_tool, &project, "index.js");
    assert!(!cold.is_hit());

    let second_tool = reopen(&directory);
    let (from_second, warm) = resolve(&second_tool, &project, "index.js");

    assert_eq!(warm, CacheStatus::Hit);
    assert_eq!(from_first.digest(), from_second.digest());
}

#[test]
fn test_two_projects_with_identical_content_share_one_graph() {
    // Same relative entry, same content: the memo key collides on purpose and
    // the trace validates, so the second project reuses the first's graph.
    let first = app();
    let second = app();
    let (_directory, cache) = cache();

    let cold = resolve_cached(
        &cache,
        &first.root(),
        &entry("index.js"),
        &ResolverOptions::default(),
    )
    .unwrap();
    let warm = resolve_cached(
        &cache,
        &second.root(),
        &entry("index.js"),
        &ResolverOptions::default(),
    )
    .unwrap();

    assert_eq!(warm.status, CacheStatus::Hit);
    assert_eq!(cold.graph.digest(), warm.graph.digest());
    assert_eq!(cold.output, warm.output);
}

#[test]
fn test_two_projects_with_different_content_do_not_collide() {
    let first = app();
    let second = app();
    second.write("transitive.js", "export const value = 99;\n");
    let (_directory, cache) = cache();

    resolve(&cache, &first, "index.js");
    let (_, status) = resolve(&cache, &second, "index.js");

    assert_eq!(
        status,
        CacheStatus::Miss(MissReason::InputChanged(entry("transitive.js")))
    );
}

#[test]
fn test_collecting_the_graph_object_misses_and_recovers() {
    let project = app();
    let (_directory, cache) = cache();

    let cold = resolve_cached(
        &cache,
        &project.root(),
        &entry("index.js"),
        &ResolverOptions::default(),
    )
    .unwrap();

    // GC with an empty live set: the record survives, its graph does not.
    gc::collect(cache.cas(), &BTreeSet::new(), &GcOptions::default()).unwrap();
    let (_, status) = resolve(&cache, &project, "index.js");
    assert_eq!(
        status,
        CacheStatus::Miss(MissReason::OutputEvicted(cold.output))
    );

    let (_, status) = resolve(&cache, &project, "index.js");
    assert_eq!(status, CacheStatus::Hit);
}

#[test]
fn test_live_outputs_keep_graphs_through_gc() {
    let project = app();
    let (_directory, cache) = cache();

    resolve(&cache, &project, "index.js");
    let live = cache.live_outputs().unwrap();
    assert_eq!(live.len(), 1);

    let report = gc::collect(cache.cas(), &live, &GcOptions::default()).unwrap();
    assert_eq!(report.objects_removed, 0);

    let (_, status) = resolve(&cache, &project, "index.js");
    assert_eq!(status, CacheStatus::Hit);
}
