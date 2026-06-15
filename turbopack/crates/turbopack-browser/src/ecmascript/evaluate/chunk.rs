use std::io::Write;

use anyhow::Result;
use either::Either;
use indoc::formatdoc;
use serde::Serialize;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ResolvedVc, TryJoinIterExt, ValueToString, Vc, turbobail};
use turbo_tasks_fs::{File, FileContent, FileSystemPath};
use turbopack_core::{
    asset::{Asset, AssetContent},
    chunk::{
        ChunkData, ChunkingContext, ChunksData, EvaluatableAssets, MinifyType,
        ModuleChunkItemIdExt, ModuleId,
    },
    code_builder::{Code, CodeBuilder},
    ident::AssetIdent,
    module::Module,
    output::{OutputAsset, OutputAssets, OutputAssetsReference, OutputAssetsWithReferenced},
    source_map::{GenerateSourceMap, SourceMapAsset},
};
use turbopack_ecmascript::{
    chunk::{EcmascriptChunkData, EcmascriptChunkPlaceable},
    minify::minify,
    utils::StringifyJs,
};

use crate::{
    BrowserChunkingContext,
    chunking_context::{CURRENT_CHUNK_METHOD_DOCUMENT_CURRENT_SCRIPT_EXPR, CurrentChunkMethod},
};

/// An Ecmascript chunk that registers an entrypoint's chunks and runtime module
/// IDs onto the `globalThis[TURBOPACK]` queue, which the shared
/// [`super::runtime::EcmascriptBrowserRuntimeChunk`] drains.
#[turbo_tasks::value(shared)]
#[derive(ValueToString)]
#[value_to_string("Ecmascript Browser Evaluate Chunk")]
pub(crate) struct EcmascriptBrowserEvaluateChunk {
    chunking_context: ResolvedVc<BrowserChunkingContext>,
    ident: ResolvedVc<AssetIdent>,
    other_chunks: ResolvedVc<OutputAssets>,
    evaluatable_assets: ResolvedVc<EvaluatableAssets>,
}

#[turbo_tasks::value_impl]
impl EcmascriptBrowserEvaluateChunk {
    /// Creates a new [`Vc<EcmascriptBrowserEvaluateChunk>`].
    #[turbo_tasks::function]
    pub fn new(
        chunking_context: ResolvedVc<BrowserChunkingContext>,
        ident: ResolvedVc<AssetIdent>,
        other_chunks: ResolvedVc<OutputAssets>,
        evaluatable_assets: ResolvedVc<EvaluatableAssets>,
    ) -> Vc<Self> {
        EcmascriptBrowserEvaluateChunk {
            chunking_context,
            ident,
            other_chunks,
            evaluatable_assets,
        }
        .cell()
    }

    #[turbo_tasks::function]
    async fn chunks_data(&self) -> Result<Vc<ChunksData>> {
        Ok(ChunkData::from_assets(
            self.chunking_context.output_root().owned().await?,
            *self.other_chunks,
        ))
    }

    /// The params for bootstrapping: `{ otherChunks, runtimeModuleIds }`. This
    /// describes which other chunks must load and which runtime modules to instantiate.
    ///
    /// The emitted evaluate-chunk file ([`Self::code`]) reuses these same params,
    /// wrapping them as `push([selfPath, params])`. Next.js inlines them in production.
    #[turbo_tasks::function]
    pub(crate) async fn chunk_group_bootstrap_params(self: Vc<Self>) -> Result<Vc<RcStr>> {
        let this = self.await?;

        let other_chunks_data = self.chunks_data().await?;
        let other_chunks_data = other_chunks_data.iter().try_join().await?;
        let other_chunks_data: Vec<_> = other_chunks_data
            .iter()
            .map(|chunk_data| EcmascriptChunkData::new(chunk_data))
            .collect();

        let runtime_module_ids = this
            .evaluatable_assets
            .await?
            .iter()
            .map({
                let chunking_context = this.chunking_context;
                move |entry| async move {
                    if let Some(placeable) =
                        ResolvedVc::try_sidecast::<Box<dyn EcmascriptChunkPlaceable>>(*entry)
                    {
                        Ok(Some(
                            placeable
                                .chunk_item_id(Vc::upcast(*chunking_context))
                                .await?,
                        ))
                    } else {
                        Ok(None)
                    }
                }
            })
            .try_join()
            .await?
            .into_iter()
            .flatten()
            .collect();

        let params = EcmascriptBrowserChunkRuntimeParams {
            other_chunks: &other_chunks_data,
            runtime_module_ids,
        };

        Ok(Vc::cell(format!("{}", StringifyJs(&params)).into()))
    }

    #[turbo_tasks::function]
    async fn code(self: Vc<Self>) -> Result<Vc<Code>> {
        let this = self.await?;
        let source_maps = *this
            .chunking_context
            .reference_chunk_source_maps(Vc::upcast(self))
            .await?;

        let params = self.chunk_group_bootstrap_params().await?;
        let chunk_loading_global = this.chunking_context.chunk_loading_global().await?;
        // `||=` would be better but we need to be es2020 compatible.
        // `x || (x = default)` avoids _writing_ the property in the common case.
        let use_string_literal = matches!(
            *this.chunking_context.current_chunk_method().await?,
            CurrentChunkMethod::StringLiteral
        );
        // Lifetime hack to keep the path read alive for the borrow below.
        let chunk_path;
        let script_or_path = if use_string_literal {
            let output_root = this.chunking_context.output_root().await?;
            chunk_path = self.path().await?;
            let chunk_server_path = if let Some(path) = output_root.get_path_to(&chunk_path) {
                path
            } else {
                turbobail!("chunk path {chunk_path} is not in output root {output_root}");
            };
            Either::Left(StringifyJs(chunk_server_path))
        } else {
            Either::Right(CURRENT_CHUNK_METHOD_DOCUMENT_CURRENT_SCRIPT_EXPR)
        };
        let statement = formatdoc! {
            r#"
                (globalThis[{chunk_loading_global}] || (globalThis[{chunk_loading_global}] = [])).push([
                    {script_or_path},
                    {params}
                ]);
            "#,
            chunk_loading_global = StringifyJs(&chunk_loading_global),
            script_or_path = script_or_path,
            params = &**params,
        };

        let mut code = CodeBuilder::new(
            source_maps,
            *this.chunking_context.debug_ids_enabled().await?,
        );
        write!(code, "{}", statement)?;

        let mut code = code.build();

        if let MinifyType::Minify { mangle } = *this.chunking_context.minify_type().await? {
            code = minify(code, source_maps, mangle)?;
        }

        Ok(code.cell())
    }

    #[turbo_tasks::function]
    async fn ident_for_path(&self) -> Result<Vc<AssetIdent>> {
        let mut ident = self
            .ident
            .owned()
            .await?
            .with_modifier(rcstr!("ecmascript browser evaluate chunk"));

        let evaluatable_assets = self.evaluatable_assets.await?;
        ident.modifiers.extend(
            evaluatable_assets
                .iter()
                .map(|entry| entry.ident().to_string().owned())
                .try_join()
                .await?,
        );
        ident.modifiers.extend(
            self.other_chunks
                .await?
                .iter()
                .map(|chunk| chunk.path().to_string().owned())
                .try_join()
                .await?,
        );

        Ok(ident.into_vc())
    }

    #[turbo_tasks::function]
    async fn source_map(self: Vc<Self>) -> Result<Vc<SourceMapAsset>> {
        let this = self.await?;
        Ok(SourceMapAsset::new(
            Vc::upcast(*this.chunking_context),
            self.ident_for_path(),
            Vc::upcast(self),
        ))
    }
}

#[turbo_tasks::value_impl]
impl OutputAssetsReference for EcmascriptBrowserEvaluateChunk {
    #[turbo_tasks::function]
    async fn references(self: Vc<Self>) -> Result<Vc<OutputAssetsWithReferenced>> {
        let this = self.await?;
        let mut references = Vec::new();

        let include_source_map = *this
            .chunking_context
            .reference_chunk_source_maps(Vc::upcast(self))
            .await?;

        if include_source_map {
            references.push(ResolvedVc::upcast(self.source_map().to_resolved().await?));
        }

        references.extend(this.other_chunks.await?.iter().copied());

        Ok(OutputAssetsWithReferenced::from_assets(Vc::cell(
            references,
        )))
    }
}

#[turbo_tasks::value_impl]
impl OutputAsset for EcmascriptBrowserEvaluateChunk {
    #[turbo_tasks::function]
    async fn path(self: Vc<Self>) -> Result<Vc<FileSystemPath>> {
        let this = self.await?;
        let ident = self.ident_for_path();
        Ok(this.chunking_context.chunk_path(
            Some(Vc::upcast(self)),
            ident,
            Some(rcstr!("turbopack")),
            rcstr!(".js"),
        ))
    }
}

#[turbo_tasks::value_impl]
impl Asset for EcmascriptBrowserEvaluateChunk {
    #[turbo_tasks::function]
    async fn content(self: Vc<Self>) -> Result<Vc<AssetContent>> {
        Ok(AssetContent::file(
            FileContent::Content(File::from(
                self.code()
                    .to_rope_with_magic_comments(|| self.source_map())
                    .await?,
            ))
            .cell(),
        ))
    }
}

#[turbo_tasks::value_impl]
impl GenerateSourceMap for EcmascriptBrowserEvaluateChunk {
    #[turbo_tasks::function]
    fn generate_source_map(self: Vc<Self>) -> Vc<FileContent> {
        self.code().generate_source_map()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EcmascriptBrowserChunkRuntimeParams<'a, T> {
    /// Other chunks in the chunk group this chunk belongs to, if any. Does not
    /// include the chunk itself.
    ///
    /// These chunks must be loaed before the runtime modules can be
    /// instantiated.
    other_chunks: &'a [T],
    /// List of module IDs that this chunk should instantiate when executed.
    runtime_module_ids: Vec<ModuleId>,
}
