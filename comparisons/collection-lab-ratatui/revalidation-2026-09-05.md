# Combined Comparison Revalidation

This report records one successful serial run on **2026-09-05**, after combining the code fixes for #22-#26.
The [complete JSON dataset](revalidation-2026-09-05.json) retains **108 Criterion MEAN estimates and 95% confidence intervals**,
**84 output records**, **142 memory records**, and **42 ArborUI phase entries**. Tables in the [comparison README](README.md#measurement-status)
and [application proof](../../research/library/experiments/application-proof.md#measurement-status) remain historical, not current results or a before/after control.

## Provenance

The measured source was a pure combined Git tree, not the #25 branch alone:

| Source                         | Exact Git Object                           |
| ------------------------------ | ------------------------------------------ |
| Base commit                    | `d0a8ea442a92d734ce9266b282cf9d74636fece7` |
| Measured tree                  | `a860695ad7201334ee84ad83b9cc6dde5b0b4a7f` |
| #22 startup sizing             | `82a365cdc8725bd2e2e9a5ba294f391ba67967b8` |
| #23 active-item paging         | `ad5a45fff562e5193be2cc7275e905ba18177fe1` |
| #24 overlay styles             | `f06f1baaf9014f2cc394a811735cc6a190cf025c` |
| #25 bounded benchmark history  | `26f87ca96c11ef30dabe792d1635edb6c75f6ea3` |
| #26 controlled table scrolling | `e9d513e697615c38c1eaf3f33a2895bcfc6151a5` |

The source/artifact root was `/var/folders/6t/yjbwmnq95_75738t55nwk0m80000gn/T/opencode/arborui-batch-4-integration`.
Authoritative inputs are `target/revalidation/{README.md,criterion.log,output.log,memory.log,phase.log,*started.txt,*ended.txt}`
and `target/comparisons/collection-lab-ratatui/criterion/**/combined-22-26/{benchmark.json,estimates.json,sample.json}`.
The JSON includes source-relative paths, log/metadata SHA-256 hashes, and an aggregate digest of all 324 Criterion input files.
Raw local artifacts are not duplicated here; the dataset preserves their numerical results and provenance.

| Environment             | Value                                                        |
| ----------------------- | ------------------------------------------------------------ |
| OS / architecture       | macOS 26.6.2 (25G83), Darwin arm64                           |
| CPU / memory            | Apple M5 Pro, 18 logical CPUs; 51,539,607,552 bytes (48 GiB) |
| Comparison compiler     | rustc 1.88.0 (6b00bc388 2025-06-23), LLVM 20.1.5             |
| Libraries               | Criterion 0.7.0, Ratatui 0.30.2, DHAT 0.3.3                  |
| Root / comparison Taffy | 0.12.1 / 0.12.2, separate locked dependency graphs           |
| Criterion UTC window    | 20:21:31-20:42:47, 1,276 seconds (21m16s)                    |
| Serial probe UTC window | 20:44:12-20:46:28, 136 seconds (2m16s)                       |

The root Rust 1.85.0 MSRV gate passed separately; **the measured comparison used 1.88.0**, not that root toolchain or dependency graph. No lockfile was updated.
Root lock SHA-256: `d8d2ceae9a89057a1718b2ad0ac1fbf5608946cf4dc18bdac30a2e26294fd1ce`.
Comparison lock SHA-256: `e6d26a5f1e1f5761cd6ba0a0f02907045cdabb7ca3a31ee11ea6d962a3b52b9d`.

The run operator recorded that temporary startup/paging/no-history and table-wheel/paging/selection cross-tests passed, then were **moved out before measurement**.
`git diff --exit-code` and `git write-tree` checks established unchanged measured source before and after all probes.
Preflight checks, smoke validation, and release-target compilation finished first. No other build, test, or measurement command
**from that session** overlapped the measurements; this does not establish that the global host was idle.
These preflight and host facts are operator provenance, distinct from the directly parsed result logs.

## Commands And Policy

The actual Criterion command, from the source root, was:

```sh
env -u NO_COLOR -u FORCE_COLOR -u CLICOLOR -u CLICOLOR_FORCE \
  CARGO_TERM_COLOR=never CARGO_BUILD_JOBS=2 \
  CARGO_TARGET_DIR="$PWD/target/comparisons/collection-lab-ratatui" \
  cargo +1.88.0 bench \
  --manifest-path comparisons/collection-lab-ratatui/Cargo.toml --locked \
  --bench application_turns -- --noplot --save-baseline combined-22-26
```

Then, sequentially, the same four color variables were unset for `just comparison-output-metrics`, `just comparison-memory-metrics`, and
`just comparison-phase-metrics`, with `CARGO_TERM_COLOR=never`, `CARGO_BUILD_JOBS=2`, and `RUST_TEST_THREADS=1`. Exact commands are in the JSON.
**`NO_COLOR` affects the real serializer**; earlier `NO_COLOR=1` output numbers are not used. Cargo diagnostic color control does not disable serializer color.

Criterion used unchanged normal settings: 3-second warm-up, 5-second measurement target, 100 samples, 100,000 resamples, 95% confidence, quick mode off.
All 108 unique IDs reached `Analyzing`; every saved `sample.json` has exactly 100 `iters` and 100 `times`.
These are samples containing repeated iterations, not merely 100 application turns. Output passed 6 tests in 2.14s;
memory passed 1 matrix test covering 142 isolated processes in 130.72s; phase passed 6 tests in 2.14s.

## Measurement Boundaries

- Criterion measures complete logical application turns through the respective test backends, not isolated renderers or production ANSI output.
  Persistent fixtures prebuild the model and settle before timing; explicit resets are untimed. Cold includes model/harness creation, first draw, and scope destruction.
- All ArborUI timing and `TestApp` memory fixtures disable patch recording **before initial settlement**, skipping the clone/history push, not merely clearing outside timing.
  Validation, committed frames, recovery, and settle counters remain enabled. Ordinary tests and output probes keep recording.
- Collection line navigation times one ArborUI loop but accumulates individual Ratatui turn intervals. Other groups follow their `iter`/`iter_custom` paths in
  [the benchmark](benches/application_turns.rs). Timer placement is not identical.
- Action collections, tables, and logs start at 100,000 items; navigation also tests 1,000 and 1,000,000. Appends advance a bounded model without resetting it
  (configured limit 1,100,000); table updates advance revision. Reverse retains the shared O(n) model/provider update. Model storage remains O(n).
- Collection/table/log base size is 48x12, resizing to 48x16; overlay is 40x12, resizing to 44x14; Unicode is 36x10, narrowing to 30x10.
  Storm traces and their starting state are in the [contract](README.md#resize-storm-result) and JSON.
- A storm measurement is **eight complete resize turns**, not one turn; all storm means, allocation/output rows, and phase totals retain that boundary.
  Criterion's storm throughput is eight elements per iteration.
- No actual PTY latency, terminal lifecycle, producer sleep/scheduling, ingress latency, terminal-emulator paint time, or transport latency is included.

## Criterion Means

Every table cell below is **MEAN [95% CI lower, upper] in microseconds**, rounded to three decimal places only for display.
The JSON preserves original decimal numbers in nanoseconds, including the mean standard error. These are explicitly `estimates.json.mean`,
**not** Criterion's usual console `time:` slope point estimate. For example, overlay focus-next ArborUI mean is 26,078.938381573254 ns,
while its slope is 26,074.0838628639 ns. Intervals quantify uncertainty in the mean for this run, not per-turn percentiles or host-to-host variability.

### Collections

| Scenario / Mode / Items          |        ArborUI MEAN [95% CI], us |        Ratatui MEAN [95% CI], us |
| -------------------------------- | -------------------------------: | -------------------------------: |
| line-navigation/fixed/1000       |          21.146 [20.997, 21.329] |             7.096 [7.051, 7.150] |
| line-navigation/variable/1000    |          24.522 [24.485, 24.559] |             8.263 [8.237, 8.290] |
| line-navigation/fixed/100000     |          21.060 [20.978, 21.174] |             7.065 [7.046, 7.086] |
| line-navigation/variable/100000  |          24.822 [24.748, 24.902] |             8.190 [8.172, 8.208] |
| line-navigation/fixed/1000000    |          21.023 [20.948, 21.121] |             7.057 [7.044, 7.071] |
| line-navigation/variable/1000000 |          24.640 [24.591, 24.697] |             8.235 [8.197, 8.282] |
| cold-initial-render/fixed        | 14910.921 [14877.017, 14952.118] | 15098.041 [15046.626, 15157.762] |
| cold-initial-render/variable     | 15134.868 [15089.900, 15189.451] | 15037.259 [15019.351, 15056.653] |
| page-down/fixed                  |          48.224 [47.543, 49.113] |             7.069 [7.040, 7.102] |
| page-down/variable               |          44.359 [44.080, 44.691] |             8.397 [8.332, 8.492] |
| end/fixed                        |          55.221 [55.002, 55.479] |             7.472 [7.415, 7.540] |
| end/variable                     |          63.063 [62.547, 63.725] |             8.390 [8.356, 8.431] |
| resize/fixed                     |          76.699 [76.451, 76.979] |          11.897 [11.865, 11.931] |
| resize/variable                  |          81.882 [81.677, 82.151] |          13.590 [13.568, 13.620] |
| selection/fixed                  |          20.908 [20.829, 21.013] |             7.068 [7.042, 7.099] |
| selection/variable               |          24.716 [24.599, 24.867] |             8.200 [8.171, 8.231] |
| reverse/fixed                    |       361.549 [359.816, 363.490] |       340.673 [339.523, 341.765] |
| reverse/variable                 |       366.480 [364.890, 368.261] |       342.850 [341.632, 344.165] |
| unchanged-redraw/fixed           |          11.540 [11.492, 11.610] |             6.639 [6.605, 6.679] |
| unchanged-redraw/variable        |          10.384 [10.366, 10.405] |             7.535 [7.506, 7.576] |

### Table And Log

| Workload / Scenario / Items   |        ArborUI MEAN [95% CI], us |        Ratatui MEAN [95% CI], us |
| ----------------------------- | -------------------------------: | -------------------------------: |
| table/line-navigation/1000    |          42.123 [42.069, 42.183] |       153.090 [152.733, 153.598] |
| table/line-navigation/100000  |          42.536 [42.232, 42.913] |       153.451 [152.969, 154.063] |
| table/line-navigation/1000000 |          42.528 [42.436, 42.629] |       156.498 [155.379, 158.167] |
| table/cold-initial-render     |    7434.902 [7393.370, 7480.628] |    7354.234 [7315.921, 7400.193] |
| table/page-down               |          97.754 [97.225, 98.414] |       159.720 [158.774, 160.758] |
| table/selection               |          43.661 [43.237, 44.299] |       154.979 [154.393, 155.648] |
| table/resize                  |       157.583 [156.561, 158.971] |       165.439 [164.836, 166.107] |
| table/visible-update          |          83.309 [83.098, 83.558] |       153.865 [153.260, 154.643] |
| table/offscreen-update        |          28.097 [28.004, 28.204] |       154.367 [153.889, 154.900] |
| log/line-scrolling/1000       |          77.535 [76.983, 78.271] |          10.854 [10.779, 10.957] |
| log/line-scrolling/100000     |          76.793 [76.516, 77.111] |          10.914 [10.864, 10.982] |
| log/line-scrolling/1000000    |          76.744 [76.408, 77.149] |          10.827 [10.800, 10.860] |
| log/cold-initial-render       | 12045.616 [11983.133, 12114.190] | 11880.251 [11844.915, 11919.551] |
| log/page-up                   |          90.941 [90.552, 91.382] |          10.986 [10.928, 11.047] |
| log/resize                    |       121.113 [120.509, 121.789] |          17.581 [17.553, 17.613] |
| log/append-following          |          75.321 [74.762, 75.925] |          11.054 [11.034, 11.075] |
| log/append-paused             |          14.492 [14.404, 14.586] |             9.617 [9.547, 9.689] |

### Overlay And Unicode

| Workload / Scenario           | ArborUI MEAN [95% CI], us | Ratatui MEAN [95% CI], us |
| ----------------------------- | ------------------------: | ------------------------: |
| overlay/cold-initial-render   |   56.845 [56.465, 57.357] |      8.597 [8.554, 8.652] |
| overlay/open                  |   63.572 [63.424, 63.738] |      9.910 [9.894, 9.930] |
| overlay/focus-next            |   26.079 [26.037, 26.126] |      7.808 [7.784, 7.833] |
| overlay/cancel                |   41.559 [41.511, 41.611] |      7.263 [7.258, 7.268] |
| overlay/confirm               |   41.566 [41.495, 41.664] |      7.334 [7.307, 7.367] |
| overlay/background-activation |      7.990 [7.922, 8.088] |      4.946 [4.933, 4.961] |
| overlay/resize-open           |   66.191 [65.909, 66.568] |   12.789 [12.768, 12.810] |
| unicode/cold-initial-render   |   70.263 [70.216, 70.314] |   13.098 [12.971, 13.251] |
| unicode/shift-boundary        |   36.497 [36.419, 36.590] |      8.796 [8.741, 8.863] |
| unicode/replace-wide          |   46.604 [46.460, 46.805] |      9.620 [9.598, 9.649] |
| unicode/resize-narrow         |   51.262 [51.120, 51.441] |   10.442 [10.347, 10.569] |

### Eight-Turn Storms

| Workload     | ArborUI MEAN [95% CI], us/storm | Ratatui MEAN [95% CI], us/storm |
| ------------ | ------------------------------: | ------------------------------: |
| fixed        |      552.861 [549.757, 557.594] |         79.401 [79.218, 79.612] |
| variable     |      593.910 [591.429, 596.854] |         90.378 [90.070, 90.745] |
| table        |   1235.966 [1232.832, 1239.712] |   1272.917 [1268.519, 1278.174] |
| log-paused   |      842.347 [837.441, 848.178] |      109.141 [108.786, 109.627] |
| overlay-open |      510.478 [507.036, 514.669] |         95.252 [94.709, 95.910] |
| unicode      |      433.914 [429.732, 438.872] |         92.783 [92.260, 93.370] |

Navigation means remain approximately flat over three logical sizes, consistent with the shared application-owned visible-range policy.
On this run, ArborUI's table action means are lower than the matched Ratatui table adapter's, while Ratatui's collection/log/overlay/Unicode action means are lower.
Cold table intervals overlap. These workload-specific observations establish no global framework winner and no improvement percentage against the historical WSL host
or against patch recording: host, combined code, and measurement policy differ.

## Memory Evidence

DHAT runs one release process per case. `model` profiles generated data/providers; `initial-render` builds that model **before** profiling;
`cold` includes it; actions prebuild a settled fixture. Retained bytes are allocations **made during profiling and still live** before result drop,
not total RSS, net size change, or proof of a leak. All 142 cases passed the [`assert_released` checks](tests/memory_metrics.rs):
end blocks and bytes were zero after dropping each result. These end values are asserted before printing, not separate columns in the log.
There are no memory confidence intervals.

The shared model retained bytes are unchanged in policy and linear in item count:

| Model, Both Frameworks  | 1,000 Items | 100,000 Items | 1,000,000 Items |
| ----------------------- | ----------: | ------------: | --------------: |
| Collection, either mode |      148987 |      14899987 |       148999987 |
| Table                   |      152000 |      15200000 |       152000000 |
| Log                     |       96000 |       9600000 |        96000000 |

First-render bytes below exclude that model. Triplets are **1k / 100k / 1m**:

| Framework / Workload         | Allocated Bytes          | Peak Bytes               | Retained Bytes           |
| ---------------------------- | ------------------------ | ------------------------ | ------------------------ |
| ArborUI fixed                | 275429 / 275429 / 275429 | 183476 / 183476 / 183476 | 90932 / 90932 / 90932    |
| ArborUI variable             | 251926 / 251926 / 251926 | 177924 / 177924 / 177924 | 85380 / 85380 / 85380    |
| ArborUI table                | 743370 / 743370 / 743370 | 290412 / 290412 / 290412 | 197868 / 197868 / 197868 |
| ArborUI log                  | 286182 / 286206 / 286182 | 187196 / 187220 / 187196 | 94652 / 94676 / 94652    |
| Ratatui fixed, variable, log | 82944 / 82944 / 82944    | 82944 / 82944 / 82944    | 82944 / 82944 / 82944    |
| Ratatui table                | 296588 / 292292 / 293764 | 165932 / 162432 / 164232 | 82944 / 82944 / 82944    |

This supports viewport-bounded first-frame state at these sizes, not identical allocation columns everywhere: ArborUI log varies by 24 bytes,
and Ratatui table transient allocations/peaks vary despite equal retention. The matrix's boundedness assertions permit limited growth;
the exact triplets above are observations. Overlay model retention is zero and Unicode is 396 bytes on both sides.

Selected action cells are **allocated bytes / retained bytes** at 100,000 items (overlay uses its single model); all allocation/block columns remain in JSON:

| Workload / Action        |           ArborUI |           Ratatui |
| ------------------------ | ----------------: | ----------------: |
| Fixed / unchanged-redraw |     61684 / 44620 |             0 / 0 |
| Table / page-down        |   473622 / 222852 |        219020 / 0 |
| Log / append-following   | 6574406 / 6479748 | 6400120 / 6400064 |
| Log / append-paused      | 6467732 / 6449164 | 6400120 / 6400064 |
| Overlay / focus-next     |     70861 / 40236 |           113 / 0 |

Log append includes shared application deque capacity growth; it is not all framework memory. More importantly, removal of **unbounded patch history**
is established by [repeated-turn retention tests](tests/retention.rs), including 100 storms and action/reset traces with empty history,
preserved frames/focus, and bounded model capacities. A single DHAT retained-byte row cannot prove that.

## Output And Parity

The debug-profile output probe uses real production Crossterm serializers with ANSI16 capabilities, separately from Criterion.
Selected cells are exact **bytes / writer callbacks / flushes**; callbacks are **not syscalls**:

| Workload / Scenario      |           ArborUI |           Ratatui |
| ------------------------ | ----------------: | ----------------: |
| Fixed / initial-render   |   5269 / 3723 / 1 |     861 / 542 / 1 |
| Table / visible-update   |      102 / 68 / 1 |       65 / 44 / 1 |
| Table / offscreen-update |         0 / 0 / 0 |       19 / 12 / 1 |
| Overlay / open           |   4622 / 3242 / 1 |    1012 / 713 / 1 |
| Overlay / focus-next     |     226 / 152 / 1 |       74 / 52 / 1 |
| Overlay / resize-open    |   5796 / 4059 / 1 |    1166 / 860 / 2 |
| Table / eight-turn storm | 46809 / 32796 / 8 | 10216 / 6927 / 16 |

All six storms use eight ArborUI flushes versus sixteen Ratatui flushes because Ratatui preserves production clear plus draw on resize.
Fixed/variable unchanged redraw, paused log append, and overlay background activation also emit 0/0/0 versus 19/12/1. Zero output does not mean zero update/reconcile work.

The [overlay regression at #24][overlay-test] asserts **20 changed styled cells on each side** for focus-next.
Full styled-cell parity is established for overlay traces, not all workloads. [Table scrolling assertions at #26][table-test] check semantic/character parity
and specific ID-cell foreground/background/bold styles; table gap/focus styling intentionally differs.
The new mouse-wheel parity test is functional coverage, **not an added case in this Criterion timing matrix**.

## ArborUI Phases

These are separate release-mode **AppRunner headless** measurements, not `TestApp`, not Criterion, and not a direct Ratatui phase comparison.
The probe prints integer average nanoseconds from 20 initial renders or 100 action/storm samples, with no confidence intervals.
Initial render excludes prebuilt model/runner construction. **Update is separate from Render total**; the latter is timed independently
and need not equal the sum of printed subphases. JSON preserves all nine columns.

| Workload / Scenario        | Update ns | Layout ns | Paint ns | Render Total ns |
| -------------------------- | --------: | --------: | -------: | --------------: |
| Fixed / reverse            |    344115 |      5235 |    26226 |           46969 |
| Variable / reverse         |    345301 |      4292 |    29850 |           48607 |
| Table / offscreen-update   |       220 |         0 |      741 |           28534 |
| Overlay / focus-next       |      4889 |         0 |    18067 |           38323 |
| Log / append-paused        |       344 |         0 |      577 |           14558 |
| Table / eight-turn storm   |     36773 |    429840 |   332197 |         1091747 |
| Unicode / eight-turn storm |      8451 |     38708 |   203602 |          340046 |

Reverse's O(n) update remains distinct from rendering. No-layout paths remain visible for selection/focus and unchanged projections.
Table storm layout exceeds paint, while collection/log/overlay/Unicode storms spend more time in paint than layout in this phase run.
This attribution selects no universal optimization.

## Artifact Verification

Documentation preparation ran no benchmarks or probes. Deterministic Node processing matched every mean/CI to its saved baseline,
preserved numeric lexemes without rounding, checked all 108 sample counts and IDs, and compared every probe row and success summary.
It also checked lock/log hashes and all 54 displayed timing pairs against the JSON. `NO_COLOR=1 just fmt-check` and `git diff --check`
validate documentation only; that environment was **not** used for measurements.

To independently check numerical extraction using Node 24 from the repository root, set `ARTIFACT_SOURCE` to the source/artifact root above
and run this module (read-only; no build or measurement commands):

```js
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
const read = (p) => readFileSync(p, "utf8");
const source = (p) => read(`${process.env.ARTIFACT_SOURCE}/${p}`);
const path = "comparisons/collection-lab-ratatui/revalidation-2026-09-05.json";
const data = JSON.parse(read(path));
const exact = (s) =>
  JSON.parse(s, (_, v, c) => (typeof v === "number" ? c.source : v));
const lexemes = exact(read(path));
const root = data.provenance.criterion_artifact_root;
assert.equal(data.criterion.length, 108);
const ids = [
  ...source("target/revalidation/criterion.log").matchAll(
    /^Benchmarking (.+): Analyzing$/gm,
  ),
].map((m) => m[1]);
assert.equal(new Set(ids).size, 108);
const savedIds = data.criterion.map((r) => r.benchmark.full_id);
assert.deepEqual(savedIds, ids);
for (const [i, row] of data.criterion.entries()) {
  const dir = `${root}/${row.benchmark.directory_name}/combined-22-26`;
  assert.deepEqual(JSON.parse(source(`${dir}/benchmark.json`)), row.benchmark);
  assert.deepEqual(
    exact(source(`${dir}/estimates.json`)).mean,
    lexemes.criterion[i].mean_ns,
  );
  const sample = JSON.parse(source(`${dir}/sample.json`));
  assert.equal(sample.iters.length, 100);
  assert.equal(sample.times.length, row.sample_count);
  assert.equal(row.sample_count, 100);
  assert.equal(sample.sampling_mode, row.sampling_mode);
}
for (const [name, count] of [
  ["output", 84],
  ["memory", 142],
  ["phase", 42],
]) {
  const tests = [];
  let current;
  const log = source(`target/revalidation/${name}.log`);
  const cells = (s) =>
    s
      .split("|")
      .slice(1, -1)
      .map((v) => v.trim());
  for (const line of log.split("\n")) {
    const start = line.match(/^test (\w+) \.\.\. (\|.*)$/);
    if (start)
      tests.push(
        (current = { name: start[1], columns: cells(start[2]), rows: [] }),
      );
    else if (line.startsWith("|") && !line.startsWith("| ---"))
      current.rows.push(
        cells(line).map((v) => (/^\d+$/.test(v) ? Number(v) : v)),
      );
  }
  assert.deepEqual(tests, data[name].tests);
  assert.equal(
    tests.reduce((n, t) => n + t.rows.length, 0),
    count,
  );
  const summary = log.match(
    /test result: ok\. (\d+) passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in ([\d.]+)s/,
  );
  assert.equal(Number(summary[1]), data[name].tests_passed);
  assert.equal(Number(summary[2]), data[name].test_runner_elapsed_seconds);
}
for (const [p, hash] of Object.entries({
  ...data.provenance.artifact_sha256,
  ...data.provenance.lockfile_sha256,
}))
  assert.equal(createHash("sha256").update(source(p)).digest("hex"), hash);
console.log(
  "Verified 108 Criterion, 84 output, 142 memory, 42 phase records and source hashes.",
);
```

[overlay-test]: https://github.com/46ki75/arborui/blob/f06f1baaf9014f2cc394a811735cc6a190cf025c/comparisons/collection-lab-ratatui/tests/equivalence.rs
[table-test]: https://github.com/46ki75/arborui/blob/e9d513e697615c38c1eaf3f33a2895bcfc6151a5/comparisons/collection-lab-ratatui/tests/table_scroll.rs
