# TypeScript Generic Development Standards

Org-wide convention for any TypeScript codebase, independent of runtime.
Runtime-level concerns — `package.json` layout, `engines`, script naming,
lockfile choice — live in [`references/nodejs/general.md`](../nodejs/general.md)
instead; this file only covers the language and its tooling.

## Baseline `tsconfig.json`

```json
{
  "compilerOptions": {
    "target": "ES2020",
    "module": "ES2022",
    "moduleResolution": "Bundler",
    "strict": true,
    "isolatedModules": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true,
    "resolveJsonModule": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "incremental": true
  },
  "include": ["src"]
}
```

- `moduleResolution: "Bundler"` is the default choice — every observed
  project is built by a bundler (Vite, tsup), never runs `tsc` as the actual
  emitter, and `Bundler` resolution matches how the bundler itself resolves
  imports (no `.js` extension requirement on relative imports, `exports`
  map support). Use `"NodeNext"` instead only for a package that ships
  Node-runtime code with no bundling step (e.g. a CLI, a Lambda handler
  compiled straight by `tsc`) — see `references/nodejs/general.md`.
- `noEmit: true` is the norm because the bundler owns the actual build;
  `tsc --noEmit` runs as a separate typecheck step. A package that ships type
  declarations without a bundler
  (a library built with plain `tsc`) is the exception — set `noEmit: false`
  and `declaration: true` there instead.
- `strict: true` is the floor for every project. Stricter flags
  (`noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`,
  `noPropertyAccessFromIndexSignature`) are not currently used anywhere in
  the org — adopting them on a new project is a reasonable choice, but it's
  a genuinely new bar, not a codification of existing practice. Don't
  present it as required without confirming that first.

## Separate configs for multi-context packages

A package that has more than one build context (app code bundled by Vite,
Vite's own config file, or a declaration-only library build) adds a small
`tsconfig.*.json` for each extra context. Extend the baseline `tsconfig.json`
directly and invoke each config explicitly; project references add composite
build requirements that these small packages do not need.

```json
// tsconfig.node.json — build tooling (vite.config.ts, etc.), a different runtime context
{
  "extends": "./tsconfig.json",
  "compilerOptions": {
    "module": "ESNext",
    "types": ["node"]
  },
  "include": ["vite.config.ts"]
}
```

A published library adds `tsconfig.lib.json`, extending the baseline and
flipping to declaration-only emit for the built package while excluding
test and story files:

```json
// tsconfig.lib.json — the published build
{
  "extends": "./tsconfig.json",
  "compilerOptions": {
    "noEmit": false,
    "declaration": true,
    "emitDeclarationOnly": true,
    "outDir": "./lib-types"
  },
  "exclude": ["src/**/*.spec.ts", "src/**/*.spec.tsx", "src/**/*.stories.tsx"]
}
```

Single-context packages need only `tsconfig.json`. For multiple contexts,
run each relevant config from one package script; do not add a solution config
or project-reference graph unless build performance demonstrates a need:

```json
{
  "scripts": {
    "typecheck": "tsc -p tsconfig.json && tsc -p tsconfig.node.json"
  }
}
```

### Declaration build script

When publishing declarations, expose their build as a separate script from the
bundler build (naming convention: see `references/nodejs/general.md`):

```json
{
  "scripts": {
    "build.types": "tsc -p tsconfig.lib.json"
  }
}
```

Published packages include `build.types` in their `check` script so the full
CI gate verifies declaration emission as well as ordinary typechecking.

## Type-only imports: inline modifier, not a separate statement

Mark type-only bindings with the inline `type` modifier inside a normal
import/export, not with a standalone `import type { ... }` statement:

```ts
// Right
export { ElmButton, type ElmButtonProps } from "./components/form/elm-button";
import { render, type RenderOptions } from "@qwik.dev/core";

// Avoid — a separate statement for what's otherwise one import
import type { RenderOptions } from "@qwik.dev/core";
import { render } from "@qwik.dev/core";
```

This keeps a component's runtime export and its prop-type export on one
line, so adding or removing the type half of the pair doesn't touch an
unrelated import statement.

## Lint and format

- **ESLint**, flat config (`eslint.config.js`), built on `typescript-eslint`
  `recommended` plus any framework-specific plugin's recommended config
  (e.g. `eslint-plugin-qwik`, `eslint-plugin-react-hooks`). Use typed
  linting via `parserOptions: { projectService: true, tsconfigRootDir:
  import.meta.dirname }` — not a hand-pointed `project: "./tsconfig.json"`,
  which breaks the moment a package gains a second tsconfig.
- **Prettier** for formatting. Default config — no `prettier.config.js`
  unless a project has a concrete reason to deviate, same philosophy as
  `ruff` defaults on the Python side. Expose `fmt` / `fmt.check` scripts
  (see `references/nodejs/general.md` for the naming convention).
- **Stylelint** for CSS/SCSS, kept separate from ESLint rather than folded
  into it, whenever a package ships stylesheets. `stylelint-config-standard-scss`
  is the observed base config.
- Biome is not used anywhere in the org. Don't introduce it into a new
  project without discussing it first — it would be a second toolchain
  alongside ESLint+Prettier, not a replacement, until a repo actually
  migrates off both.

```json
{
  "scripts": {
    "fmt": "prettier --write ./src",
    "fmt.check": "prettier --check ./src",
    "lint": "eslint ./src",
    "lint.css": "stylelint \"src/**/*.{css,scss}\""
  }
}
```
