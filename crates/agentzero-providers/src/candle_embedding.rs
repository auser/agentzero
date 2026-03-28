//! Local embedding provider using Candle with MiniLM-L6-v2.
//!
//! Generates text embeddings entirely in-process using a small, fast sentence
//! transformer model. No API calls, no network dependencies. The model is
//! downloaded from HuggingFace Hub on first use and cached locally.
//!
//! Model: `sentence-transformers/all-MiniLM-L6-v2` (384 dimensions, ~23MB)

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use async_trait::async_trait;
use candle_core::{Device, Module, Tensor};
use candle_nn::VarBuilder;
use tokenizers::Tokenizer;
use tracing::{debug, info};

use agentzero_core::embedding::EmbeddingProvider;

const MODEL_REPO: &str = "sentence-transformers/all-MiniLM-L6-v2";
const EMBEDDING_DIM: usize = 384;

/// Local embedding provider powered by Candle + MiniLM-L6-v2.
///
/// Thread-safe via `Mutex` on the loaded model state. The model is lazily
/// loaded on first `embed()` call.
pub struct CandleEmbeddingProvider {
    inner: Mutex<Option<LoadedEmbedder>>,
    cache_dir: Option<PathBuf>,
}

struct LoadedEmbedder {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

impl CandleEmbeddingProvider {
    /// Create a new embedding provider. The model will be downloaded and
    /// loaded lazily on first use.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            cache_dir: None,
        }
    }

    /// Create with a custom cache directory for the model files.
    pub fn with_cache_dir(cache_dir: PathBuf) -> Self {
        Self {
            inner: Mutex::new(None),
            cache_dir: Some(cache_dir),
        }
    }

    fn ensure_loaded(&self) -> Result<()> {
        let mut guard = self.inner.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        if guard.is_some() {
            return Ok(());
        }

        info!("loading embedding model: {MODEL_REPO}");

        let api =
            hf_hub::api::sync::Api::new().context("failed to initialize HuggingFace Hub API")?;

        let repo = if let Some(ref cache) = self.cache_dir {
            let api = hf_hub::api::sync::ApiBuilder::new()
                .with_cache_dir(cache.clone())
                .build()
                .context("failed to build HuggingFace Hub API with custom cache")?;
            api.model(MODEL_REPO.to_string())
        } else {
            api.model(MODEL_REPO.to_string())
        };

        let tokenizer_path = repo
            .get("tokenizer.json")
            .context("failed to download tokenizer.json")?;
        let weights_path = repo
            .get("model.safetensors")
            .context("failed to download model.safetensors")?;
        let config_path = repo
            .get("config.json")
            .context("failed to download config.json")?;

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("failed to load tokenizer: {e}"))?;

        let config: BertConfig = serde_json::from_str(
            &std::fs::read_to_string(&config_path).context("failed to read config.json")?,
        )
        .context("failed to parse config.json")?;

        let device = Device::Cpu;

        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights_path], candle_core::DType::F32, &device)
                .context("failed to load model weights")?
        };

        let model =
            BertModel::load(vb, &config).context("failed to build BERT model from weights")?;

        info!(dim = EMBEDDING_DIM, "embedding model loaded");

        *guard = Some(LoadedEmbedder {
            model,
            tokenizer,
            device,
        });

        Ok(())
    }
}

impl Default for CandleEmbeddingProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EmbeddingProvider for CandleEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let text = text.to_string();
        let this_ref = &self;

        // Run inference on a blocking thread since Candle is synchronous
        let embedding = tokio::task::block_in_place(|| -> Result<Vec<f32>> {
            this_ref.ensure_loaded()?;

            let guard = this_ref.inner.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
            let loaded = guard
                .as_ref()
                .context("model not loaded after ensure_loaded")?;

            let encoding = loaded
                .tokenizer
                .encode(text.as_str(), true)
                .map_err(|e| anyhow::anyhow!("tokenization failed: {e}"))?;

            let token_ids = encoding.get_ids();
            let type_ids = encoding.get_type_ids();

            let tokens = Tensor::new(token_ids, &loaded.device)?.unsqueeze(0)?;
            let type_ids_tensor = Tensor::new(type_ids, &loaded.device)?.unsqueeze(0)?;

            let embeddings = loaded.model.forward(&tokens, &type_ids_tensor)?;

            // Mean pooling over sequence length dimension
            let (_batch, seq_len, _hidden) = embeddings.dims3()?;
            let sum = embeddings.sum(1)?;
            let mean = sum.broadcast_div(&Tensor::new(&[seq_len as f32], &loaded.device)?)?;
            let mean = mean.squeeze(0)?;

            // L2 normalize
            let norm = mean.sqr()?.sum_all()?.sqrt()?;
            let normalized = mean.broadcast_div(&norm)?;

            let result: Vec<f32> = normalized.to_vec1()?;

            debug!(dim = result.len(), "generated embedding");
            Ok(result)
        })?;

        Ok(embedding)
    }

    fn dimensions(&self) -> usize {
        EMBEDDING_DIM
    }
}

// ---------------------------------------------------------------------------
// Minimal BERT model for sentence embeddings (just enough for MiniLM-L6-v2)
// ---------------------------------------------------------------------------

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct BertConfig {
    hidden_size: usize,
    num_hidden_layers: usize,
    num_attention_heads: usize,
    intermediate_size: usize,
    #[serde(default = "default_hidden_act")]
    hidden_act: String,
    #[serde(default = "default_max_position")]
    max_position_embeddings: usize,
    #[serde(default = "default_type_vocab")]
    type_vocab_size: usize,
    vocab_size: usize,
    #[serde(default = "default_eps")]
    layer_norm_eps: f64,
}

fn default_hidden_act() -> String {
    "gelu".to_string()
}
fn default_max_position() -> usize {
    512
}
fn default_type_vocab() -> usize {
    2
}
fn default_eps() -> f64 {
    1e-12
}

struct BertModel {
    embeddings: BertEmbeddings,
    layers: Vec<BertLayer>,
    final_norm: candle_nn::LayerNorm,
}

impl BertModel {
    fn load(vb: VarBuilder, config: &BertConfig) -> Result<Self> {
        let embeddings = BertEmbeddings::load(vb.pp("embeddings"), config)?;
        let mut layers = Vec::with_capacity(config.num_hidden_layers);
        for i in 0..config.num_hidden_layers {
            layers.push(BertLayer::load(
                vb.pp(format!("encoder.layer.{i}")),
                config,
            )?);
        }
        let final_norm = candle_nn::layer_norm(
            config.hidden_size,
            config.layer_norm_eps,
            vb.pp("encoder.layer_norm"),
        )?;
        Ok(Self {
            embeddings,
            layers,
            final_norm,
        })
    }

    fn forward(&self, token_ids: &Tensor, type_ids: &Tensor) -> Result<Tensor> {
        let mut hidden = self.embeddings.forward(token_ids, type_ids)?;
        for layer in &self.layers {
            hidden = layer.forward(&hidden)?;
        }
        // Some models have a final encoder layer norm; for MiniLM it's handled per-layer
        let _ = &self.final_norm;
        Ok(hidden)
    }
}

struct BertEmbeddings {
    word_embeddings: candle_nn::Embedding,
    position_embeddings: candle_nn::Embedding,
    token_type_embeddings: candle_nn::Embedding,
    layer_norm: candle_nn::LayerNorm,
}

impl BertEmbeddings {
    fn load(vb: VarBuilder, config: &BertConfig) -> Result<Self> {
        let word_embeddings = candle_nn::embedding(
            config.vocab_size,
            config.hidden_size,
            vb.pp("word_embeddings"),
        )?;
        let position_embeddings = candle_nn::embedding(
            config.max_position_embeddings,
            config.hidden_size,
            vb.pp("position_embeddings"),
        )?;
        let token_type_embeddings = candle_nn::embedding(
            config.type_vocab_size,
            config.hidden_size,
            vb.pp("token_type_embeddings"),
        )?;
        let layer_norm = candle_nn::layer_norm(
            config.hidden_size,
            config.layer_norm_eps,
            vb.pp("LayerNorm"),
        )?;
        Ok(Self {
            word_embeddings,
            position_embeddings,
            token_type_embeddings,
            layer_norm,
        })
    }

    fn forward(&self, token_ids: &Tensor, type_ids: &Tensor) -> Result<Tensor> {
        let seq_len = token_ids.dim(1)?;
        let position_ids: Vec<u32> = (0..seq_len as u32).collect();
        let position_ids = Tensor::new(position_ids, token_ids.device())?.unsqueeze(0)?;

        let word_emb = self.word_embeddings.forward(token_ids)?;
        let pos_emb = self.position_embeddings.forward(&position_ids)?;
        let type_emb = self.token_type_embeddings.forward(type_ids)?;

        let embeddings = word_emb.add(&pos_emb)?.add(&type_emb)?;
        let embeddings = self.layer_norm.forward(&embeddings)?;
        Ok(embeddings)
    }
}

struct BertLayer {
    attention: BertAttention,
    intermediate: candle_nn::Linear,
    output: candle_nn::Linear,
    ln1: candle_nn::LayerNorm,
    ln2: candle_nn::LayerNorm,
}

impl BertLayer {
    fn load(vb: VarBuilder, config: &BertConfig) -> Result<Self> {
        let attention = BertAttention::load(vb.pp("attention"), config)?;
        let intermediate = candle_nn::linear(
            config.hidden_size,
            config.intermediate_size,
            vb.pp("intermediate.dense"),
        )?;
        let output = candle_nn::linear(
            config.intermediate_size,
            config.hidden_size,
            vb.pp("output.dense"),
        )?;
        let ln1 = candle_nn::layer_norm(
            config.hidden_size,
            config.layer_norm_eps,
            vb.pp("attention.output.LayerNorm"),
        )?;
        let ln2 = candle_nn::layer_norm(
            config.hidden_size,
            config.layer_norm_eps,
            vb.pp("output.LayerNorm"),
        )?;
        Ok(Self {
            attention,
            intermediate,
            output,
            ln1,
            ln2,
        })
    }

    fn forward(&self, hidden: &Tensor) -> Result<Tensor> {
        let attn_output = self.attention.forward(hidden)?;
        let hidden = self.ln1.forward(&attn_output.add(hidden)?)?;
        let intermediate = self.intermediate.forward(&hidden)?;
        let intermediate = gelu(&intermediate)?;
        let output = self.output.forward(&intermediate)?;
        let output = self.ln2.forward(&output.add(&hidden)?)?;
        Ok(output)
    }
}

struct BertAttention {
    query: candle_nn::Linear,
    key: candle_nn::Linear,
    value: candle_nn::Linear,
    output: candle_nn::Linear,
    num_heads: usize,
    head_dim: usize,
}

impl BertAttention {
    fn load(vb: VarBuilder, config: &BertConfig) -> Result<Self> {
        let head_dim = config.hidden_size / config.num_attention_heads;
        let query = candle_nn::linear(config.hidden_size, config.hidden_size, vb.pp("self.query"))?;
        let key = candle_nn::linear(config.hidden_size, config.hidden_size, vb.pp("self.key"))?;
        let value = candle_nn::linear(config.hidden_size, config.hidden_size, vb.pp("self.value"))?;
        let output = candle_nn::linear(
            config.hidden_size,
            config.hidden_size,
            vb.pp("output.dense"),
        )?;
        Ok(Self {
            query,
            key,
            value,
            output,
            num_heads: config.num_attention_heads,
            head_dim,
        })
    }

    fn forward(&self, hidden: &Tensor) -> Result<Tensor> {
        let (batch, seq_len, _) = hidden.dims3()?;

        let q = self.query.forward(hidden)?;
        let k = self.key.forward(hidden)?;
        let v = self.value.forward(hidden)?;

        // Reshape to (batch, num_heads, seq_len, head_dim)
        let q = q
            .reshape((batch, seq_len, self.num_heads, self.head_dim))?
            .transpose(1, 2)?;
        let k = k
            .reshape((batch, seq_len, self.num_heads, self.head_dim))?
            .transpose(1, 2)?;
        let v = v
            .reshape((batch, seq_len, self.num_heads, self.head_dim))?
            .transpose(1, 2)?;

        // Scaled dot-product attention
        let scale = (self.head_dim as f64).sqrt();
        let scores = q.matmul(&k.transpose(2, 3)?)?;
        let scores = (scores / scale)?;
        let weights = candle_nn::ops::softmax(&scores, candle_core::D::Minus1)?;
        let context = weights.matmul(&v)?;

        // Reshape back to (batch, seq_len, hidden_size)
        let context =
            context
                .transpose(1, 2)?
                .reshape((batch, seq_len, self.num_heads * self.head_dim))?;
        let output = self.output.forward(&context)?;
        Ok(output)
    }
}

/// GELU activation function.
fn gelu(x: &Tensor) -> Result<Tensor> {
    Ok(x.gelu_erf()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candle_embedding_dimensions() {
        let provider = CandleEmbeddingProvider::new();
        assert_eq!(provider.dimensions(), 384);
    }

    #[test]
    fn candle_embedding_default() {
        let provider = CandleEmbeddingProvider::default();
        assert_eq!(provider.dimensions(), 384);
    }

    #[test]
    fn candle_embedding_with_cache_dir() {
        let provider = CandleEmbeddingProvider::with_cache_dir(PathBuf::from("/tmp/test-cache"));
        assert_eq!(provider.dimensions(), 384);
    }

    #[test]
    fn bert_config_deserialize() {
        let json = r#"{
            "hidden_size": 384,
            "num_hidden_layers": 6,
            "num_attention_heads": 12,
            "intermediate_size": 1536,
            "vocab_size": 30522
        }"#;
        let config: BertConfig = serde_json::from_str(json).expect("valid config");
        assert_eq!(config.hidden_size, 384);
        assert_eq!(config.num_hidden_layers, 6);
        assert_eq!(config.hidden_act, "gelu");
        assert_eq!(config.max_position_embeddings, 512);
    }
}
