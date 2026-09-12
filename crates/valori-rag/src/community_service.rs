//! Shared community enrichment orchestration.
//!
//! The service deliberately stops at a canonical, replayable extraction plan.
//! Storage is supplied by the caller so standalone and Raft sinks can use the
//! same plan without making HTTP calls or re-running the model during replay.

use crate::community::{
    assertion_identity, resolve_canonical_entity, AssertionEvidence, CanonicalEntity,
    EntityMention, ExtractedRelationship, LlmExtractionOutput,
};
use crate::{extract_entities_via_llm, LlmConfig};

#[derive(Debug, Clone)]
pub struct AssertionDraft {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub evidence: AssertionEvidence,
    pub strength: f32,
    pub assertion_id: String,
}

#[derive(Debug)]
pub struct CommunityEnrichment {
    pub extraction: LlmExtractionOutput,
    pub assertions: Vec<AssertionDraft>,
    pub canonical_entities: Vec<CanonicalEntity>,
    pub mentions: Vec<EntityMention>,
    pub source_text_hash: String,
}

pub struct CommunityExtractionService;

impl CommunityExtractionService {
    pub async fn extract(
        text: &str,
        entity_types: &[String],
        config: &LlmConfig,
        model: Option<&str>,
        http: &reqwest::Client,
        source: Option<&str>,
    ) -> Result<CommunityEnrichment, String> {
        let extraction = extract_entities_via_llm(text, entity_types, config, model, http).await?;
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        let mut canonical_entities = Vec::new();
        let mut mentions = Vec::new();
        for entity in &extraction.entities {
            let mention_id = format!(
                "mention_{}",
                blake3::hash(format!("{}|{}", hash, entity.name).as_bytes()).to_hex()
            );
            let mention = EntityMention {
                mention_id,
                entity_id: String::new(),
                source: source.map(str::to_owned),
                document_id: None,
                chunk_id: None,
                passage_id: None,
                span_start: None,
                span_end: None,
                surface_form: entity.name.clone(),
                source_text_hash: Some(hash.clone()),
                assertion_ids: Vec::new(),
            };
            let canonical = resolve_canonical_entity(&entity.name, &entity.kind, &[], &mention);
            let mut resolved_mention = mention;
            resolved_mention.entity_id = canonical.entity_id.clone();
            canonical_entities.push(canonical);
            mentions.push(resolved_mention);
        }
        let assertions = extraction
            .relationships
            .iter()
            .map(|rel: &ExtractedRelationship| {
                let predicate = rel
                    .predicate
                    .as_deref()
                    .unwrap_or(&rel.description)
                    .to_string();
                let mut evidence = rel.evidence.clone().unwrap_or_default();
                if let Some(s) = source {
                    evidence.source.get_or_insert(s.to_string());
                }
                evidence.source_text_hash.get_or_insert(hash.clone());
                let id = assertion_identity(&hash, &rel.source, &predicate, &rel.target, &evidence);
                AssertionDraft {
                    subject: rel.source.clone(),
                    predicate,
                    object: rel.target.clone(),
                    evidence,
                    strength: rel.strength,
                    assertion_id: id,
                }
            })
            .collect();
        Ok(CommunityEnrichment {
            extraction,
            assertions,
            canonical_entities,
            mentions,
            source_text_hash: hash,
        })
    }
}
