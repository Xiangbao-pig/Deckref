# DeckRef Performance Optimization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cut peak memory on large images, reduce pan/zoom jank, remove watch-event main-thread IO spikes, and stop idle full-rate repaints—without changing DeckRef features or interactions.

**Architecture:** Keep the existing “main UI thread + watcher + tex-loader + directory loader” model. Change only the hot paths named in `docs/superpowers/specs/2026-10-19-deckref-performance-design.md`: smarter decode-to-thumbnail, dirty-flag R-Tree reuse + hit-test candidates, async watch metadata, conditional `request_repaint`.

**Tech Stack:** Rust 2021, eframe/egui 0.29, image 0.25, rstar, notify, mpsc threads (no tokio).

## Global Constraints

- Spec: `docs/superpowers/specs/2026-10-19-deckref-performance-design.md` (must be committed into the repo if missing).
- Do not change user-visible interaction semantics (`docs/INTERACTIONS.md`).
- Do not raise `MAX_CARDS`, do not add memory-budget UI, do not add tokio.
- Keep `THUMBNAIL_RESOLUTION_THRESHOLD = 2048`, `THUMBNAIL_MAX_LONGEST_EDGE = 1200`.
- Main thread must never fully decode images with longest edge > 2048.
- Update `CHANGELOG.md` and mark items done in `docs/ARCHITECTURE_PERFORMANCE_REPORT.md`.
- Prefer small commits per task; open one PR when done.
- Verify with `cargo test` and `cargo build --release` (Linux CI VM is fine if Windows-only crates are cfg-gated).

---

### Task 1: Spec + plan in repo + thumbnail decode path

**Files:**
- Create: `docs/superpowers/specs/2026-10-19-deckref-performance-design.md` (copy from uploads if present)
- Create: `docs/superpowers/plans/2026-10-19-deckref-performance.md` (this plan)
- Modify: `src/image_ops.rs`
- Test: unit tests in `src/image_ops.rs` `#[cfg(test)]` module

**Interfaces:**
- Consumes: existing `load_image`, `image_dimensions`, `THUMBNAIL_*` constants
- Produces: `load_image_thumbnail` / `load_image_rgba_for_texture` behavior that avoids full-size pixel buffers for JPEG/WebP when downscaling; other formats keep decode-then-resize fallback

- [ ] **Step 1: Add failing regression test for thumbnail size**

Add a test that writes a large synthetic JPEG (e.g. 4000×3000 solid color via `image` crate) to a temp file, calls `load_image_thumbnail(path, 1200)`, asserts longest edge ≤ 1200 and that the function succeeds. (Peak-memory assertion is optional; size assertion is required.)

- [ ] **Step 2: Run test**

`cargo test load_image_thumbnail -- --nocapture`  
Expected: pass with current code OR fail only if you assert a new helper name that does not exist yet.

- [ ] **Step 3: Implement decode-aware thumbnail loading**

Refactor so that when `image_dimensions` says longest > threshold:

1. Prefer format-specific downscale if available via `image` APIs / JPEG scaling (inspect image 0.25 for `ImageReader` + limits or jpeg decoder scale).
2. Else: stream/read and resize as today, but document fallback.
3. `load_image_rgba_for_texture` must call the improved path.

Keep public constants unchanged. Do not change return type of `load_image_rgba_for_texture` → `(Vec<u8>, u32, u32)` RGBA.

- [ ] **Step 4: Re-run tests + release build**

`cargo test` and `cargo build --release`

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers src/image_ops.rs
git commit -m "perf: downscale large JPEG/WebP during decode for thumbnails"
```

---

### Task 2: Dirty-flag R-Tree reuse + hit-test candidates

**Files:**
- Modify: `src/app.rs` (viewport cull, hit testing, animation)

**Interfaces:**
- Consumes: existing rstar AABB helpers, `VISIBLE_RATIO_THRESHOLD`, `VIEWPORT_QUERY_PADDING`
- Produces: rebuild R-Tree only when layout/screen AABBs dirty; hit-test uses spatial query then `rotated_rect_contains`

- [ ] **Step 1: Locate current bulk_load + hit-test loops in `app.rs`**

Document in the PR description the function names you change.

- [ ] **Step 2: Add dirty flag + cached R-Tree fields on `DeckRefApp`**

When cards list, view transform, display scales, pick-up, or anim deltas that affect screen AABB change → set dirty. After rebuild, clear dirty. Reuse tree when not dirty.

- [ ] **Step 3: Hit testing**

Replace full reverse scan with: query R-Tree (or last frame’s tree) around pointer with small padding → sort by stack order → first `rotated_rect_contains` wins. Preserve “frontmost wins” semantics.

- [ ] **Step 4: Manual sanity**

`cargo build --release`. Mentally verify pick-up card still always drawn; pan does not thrash rebuild every frame if only view_offset changes—if view_offset is baked into AABB, mark dirty on pan (correctness over cleverness). If AABBs are in screen space and include view_offset, pan dirties the tree; that is acceptable. Optimization win is skipping rebuild when nothing moved (idle hover).

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "perf: reuse R-Tree when screen AABBs unchanged; spatial hit-test"
```

---

### Task 3: Async watch-event metadata / dimensions

**Files:**
- Modify: `src/app.rs` (`poll_watch_events` and related)
- Optionally: `src/watcher.rs` only if event payload must shrink to path-only (prefer leaving watcher as-is)

**Interfaces:**
- Consumes: watch channel events, `image_ops::image_dimensions`, `ImageCard::from_path_with_metadata`
- Produces: main thread never blocks on dimensions/metadata for new watch paths; cards appear after async result

- [ ] **Step 1: Add channel + worker for watch IO**

On Create/Modify (current semantics): enqueue `PathBuf`. Background thread computes `(path, dimensions, file_size, modified)` and sends back. Main thread `try_recv` → `from_path_with_metadata` → push + drop animation as today. Cap queue / drop stale paths after folder switch.

- [ ] **Step 2: Preserve MAX_CARDS eviction and texture cache eviction**

Same as current after card is constructed.

- [ ] **Step 3: Build + commit**

```bash
git add src/app.rs src/watcher.rs
git commit -m "perf: move watch-event image IO off the UI thread"
```

---

### Task 4: Conditional `request_repaint`

**Files:**
- Modify: `src/app.rs` end of `run_ui` / `update`

**Interfaces:**
- Consumes: animation state, pending loaders, texture pending, interaction flags
- Produces: `request_repaint` only when work remains or input requires it; when async results arrive, call `request_repaint` so UI wakes

- [ ] **Step 1: Introduce `needs_repaint` boolean** set true for: any card anim not settled, loading cards channel open/pending, texture pending/uploads, held card, drag, address-bar edit caret blink if any, clipboard shift window active, etc.

- [ ] **Step 2: Replace unconditional `request_repaint()`** with `if needs_repaint { ctx.request_repaint(); }` (or `request_repaint_after` only if an animation would stall—prefer immediate while animating).

- [ ] **Step 3: Ensure poll paths that receive new data set needs_repaint / call request_repaint**

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "perf: request_repaint only while animating or loading"
```

---

### Task 5: Docs + final verification

**Files:**
- Modify: `CHANGELOG.md`, `docs/ARCHITECTURE_PERFORMANCE_REPORT.md`

- [ ] **Step 1: Changelog entry under 未发布** describing the four perf items (Asia/Shanghai date of the change day).

- [ ] **Step 2: Update architecture report** to mark watch-event IO, R-Tree rebuild, and request_repaint as addressed where previously open.

- [ ] **Step 3: Full `cargo test && cargo build --release`**

- [ ] **Step 4: Open PR** summarizing behavior unchanged + perf changes; link spec/plan paths.

---

## Spec coverage checklist

| Spec section | Task |
|--------------|------|
| §1 Large decode | Task 1 |
| §2 R-Tree / hit-test | Task 2 |
| §3 Watch events | Task 3 |
| §4 Repaint | Task 4 |
| Docs / verify | Task 5 |
| Out of scope (MAX_CARDS, memory UI, tokio) | Explicitly not tasked |
