import { denormalizePagePath } from '../shared/lib/page-path/denormalize-page-path'
import { normalizePagePath } from '../shared/lib/page-path/normalize-page-path'

export type BuildManifest = {
  devFiles: readonly string[]
  polyfillFiles: readonly string[]
  lowPriorityFiles: readonly string[]
  rootMainFiles: readonly string[]
  // this is a separate field for flying shuttle to allow
  // different root main files per entries/build (ideally temporary)
  // until we can stitch the runtime chunks together safely
  rootMainFilesTree: { [appRoute: string]: readonly string[] }
  pages: {
    '/_app': readonly string[]
    [page: string]: readonly string[]
  }
  // Per-page Turbopack chunk-group bootstrap params — the `{ otherChunks,
  // runtimeModuleIds }` object as a JSON string.
  pagesChunkGroupBootstrapParams?: { [page: string]: string }
  // The `globalThis[...]` chunk-loading global the runtime drains (default
  // "TURBOPACK"); used to wrap `pagesChunkGroupBootstrapParams`.
  chunkLoadingGlobal?: string
}

/**
 * Builds the inline Turbopack chunk-group bootstrap script content for the given pages:
 * one `globalThis[<global>].push(<params>)` statement per page. This seeds the runtime's
 * chunk-loading queue before the shared runtime chunk drains it, so each page's entry
 * module is instantiated (and, for pages router, registers via `__NEXT_P`).
 *
 * Returns undefined when there's nothing to inline — webpack builds, or dev where the
 * per-route evaluate chunk is emitted instead of inlining.
 */
export function getTurbopackChunkGroupBootstrap(
  buildManifest: BuildManifest,
  pages: readonly string[]
): string | undefined {
  const paramsByPage = buildManifest.pagesChunkGroupBootstrapParams
  const chunkLoadingGlobal = buildManifest.chunkLoadingGlobal
  if (!paramsByPage || !chunkLoadingGlobal) return undefined

  const g = JSON.stringify(chunkLoadingGlobal)
  const statements = pages
    .map((page) => paramsByPage[page])
    .filter(Boolean)
    .map(
      (params) =>
        `(globalThis[${g}] || (globalThis[${g}] = [])).push(${params});`
    )

  return statements.length > 0 ? statements.join('\n') : undefined
}

export function getPageFiles(
  buildManifest: BuildManifest,
  page: string
): readonly string[] {
  const normalizedPage = denormalizePagePath(normalizePagePath(page))
  let files = buildManifest.pages[normalizedPage]

  if (!files) {
    console.warn(
      `Could not find files for ${normalizedPage} in .next/build-manifest.json`
    )
    return []
  }

  return files
}
