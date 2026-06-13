import type { NextConfig } from 'next'

const appShellsEnabled = !!process.env.NEXT_TEST_ENABLE_APP_SHELLS

const nextConfig: NextConfig = {
  cacheComponents: true,
  partialPrefetching: appShellsEnabled,
  experimental: {
    instantInsights: {
      validationLevel: 'manual-warning',
    },
    ...(appShellsEnabled
      ? ({
          prefetchInlining: true,
          optimisticRouting: true,
          cachedNavigations: true,
          appShells: true,
          varyParams: true,
        } satisfies NextConfig['experimental'])
      : {}),
  },
  productionBrowserSourceMaps: true,
  typescript: {
    ignoreBuildErrors: true,
  },
}

export default nextConfig
