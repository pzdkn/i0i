use crate::domain::reader::{ReaderAsset, ReaderBlock, ReaderDocument, ReaderPage, ReaderSpan};
use crate::storage::library_store::LibraryStore;

#[derive(Clone)]
pub struct ReaderService {
    store: LibraryStore,
}

impl ReaderService {
    pub fn new(store: LibraryStore) -> Self {
        Self { store }
    }

    pub fn get_reader_document(
        &self,
        paper_id: &str,
        extraction_id: Option<&str>,
    ) -> Result<ReaderDocument, String> {
        let snapshot = self.store.get_library()?;

        let paper = snapshot
            .papers
            .into_iter()
            .find(|p| p.id == paper_id)
            .ok_or_else(|| format!("Paper not found: {paper_id}"))?;

        // Resolve active or requested source
        let source = snapshot
            .document_sources
            .into_iter()
            .find(|s| {
                paper.active_source_id.as_deref() == Some(&s.id)
                    || (paper.active_source_id.is_none() && s.paper_id == paper_id)
            })
            .filter(|s| s.status == "cached" || s.status == "remote_available");

        // Resolve active extraction, or a specific one
        let active_extraction = if let Some(id) = extraction_id {
            snapshot
                .document_extractions
                .into_iter()
                .find(|e| e.id == id)
        } else {
            snapshot.document_extractions.into_iter().find(|e| {
                paper.active_extraction_id.as_deref() == Some(&e.id)
                    || (paper.active_extraction_id.is_none()
                        && source
                            .as_ref()
                            .map(|s| s.id == e.source_id)
                            .unwrap_or(false))
            })
        };

        let (pages, blocks, spans, assets, source_text) =
            if let Some(ref extraction) = &active_extraction {
                let pages: Vec<ReaderPage> = snapshot
                    .document_pages
                    .iter()
                    .filter(|p| p.extraction_id == extraction.id)
                    .cloned()
                    .map(|p| ReaderPage {
                        page_index: p.page_index,
                        width: p.width,
                        height: p.height,
                    })
                    .collect();

                let blocks: Vec<ReaderBlock> = snapshot
                    .document_blocks
                    .iter()
                    .filter(|b| b.extraction_id == extraction.id)
                    .cloned()
                    .map(|b| ReaderBlock {
                        id: b.id,
                        page_index: b.page_index,
                        block_index: b.block_index,
                        reading_order: b.reading_order,
                        kind: b.kind,
                        text: b.text,
                        asset_id: b.asset_id,
                        source_start: b.source_start,
                        source_end: b.source_end,
                        bbox_json: b.bbox_json,
                    })
                    .collect();

                let spans: Vec<ReaderSpan> = snapshot
                    .document_spans
                    .iter()
                    .filter(|s| s.extraction_id == extraction.id)
                    .cloned()
                    .map(|s| ReaderSpan {
                        id: s.id,
                        block_id: s.block_id,
                        page_index: s.page_index,
                        text: s.text,
                        source_start: s.source_start,
                        source_end: s.source_end,
                        bbox_json: s.bbox_json,
                    })
                    .collect();

                let assets: Vec<ReaderAsset> = snapshot
                    .document_assets
                    .iter()
                    .filter(|a| a.extraction_id == extraction.id)
                    .cloned()
                    .map(|a| ReaderAsset {
                        id: a.id,
                        paper_id: a.paper_id,
                        source_id: a.source_id,
                        extraction_id: a.extraction_id,
                        asset_kind: a.asset_kind,
                        page_index: a.page_index,
                        bbox_json: a.bbox_json,
                        local_path: a.local_path,
                        caption: a.caption,
                        created_at: a.created_at,
                        updated_at: a.updated_at,
                    })
                    .collect();

                // Build source_text from blocks in reading order
                let source_text: String = blocks
                    .iter()
                    .filter_map(|b| b.text.as_deref())
                    .collect::<Vec<_>>()
                    .join("\n\n");

                (pages, blocks, spans, assets, source_text)
            } else {
                (
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    String::new(),
                )
            };

        let (source_id, pdf_local_path, pdf_source_url, pdf_error) = if let Some(s) = source {
            (s.id, s.local_path, s.source_url, s.error)
        } else {
            (format!("no-source:{paper_id}"), None, None, None)
        };

        let identifier = format!("{}:{}", paper.id, paper.venue.to_lowercase());
        let citation_key = paper.id.clone();
        let tags = paper.tags.clone();

        Ok(ReaderDocument {
            paper_id: paper.id,
            source_id,
            extraction_id: active_extraction.as_ref().map(|e| e.id.clone()),
            annotation_source_id: active_extraction
                .as_ref()
                .map(|e| e.annotation_source_id.clone()),
            title: paper.title,
            authors: paper.authors,
            venue: paper.venue,
            year: paper.year,
            identifier,
            citation_key,
            tags,
            pdf_local_path,
            pdf_source_url,
            pdf_error,
            source_text,
            pages,
            blocks,
            spans,
            assets,
            text_blocks: Vec::new(),
            paragraphs: Vec::new(),
            marks: Vec::new(),
        })
    }
}
