use eyre::Result;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::time::Duration;

const BASE_URL: &str = "https://api.dune.com/api/v1";

pub struct DuneClient {
    http: reqwest::Client,
    api_key: String,
}

#[derive(Debug, Deserialize)]
struct ExecuteResponse {
    execution_id: String,
}

#[derive(Debug, Deserialize)]
struct StatusResponse {
    state: String,
}

#[derive(Debug, Deserialize)]
struct ResultsResponse<T> {
    result: ResultBody<T>,
}

#[derive(Debug, Deserialize)]
struct ResultBody<T> {
    rows: Vec<T>,
}

impl DuneClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: api_key.into(),
        }
    }

    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("DUNE_API_KEY")
            .map_err(|_| eyre::eyre!("set DUNE_API_KEY, e.g. in a .env file"))?;
        Ok(Self::new(api_key))
    }

    async fn execute(&self, query_id: u64, params: &HashMap<String, String>) -> Result<String> {
        let url = format!("{BASE_URL}/query/{query_id}/execute");
        let resp: ExecuteResponse = self
            .http
            .post(&url)
            .header("X-Dune-API-Key", &self.api_key)
            .json(&serde_json::json!({ "query_parameters": params }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp.execution_id)
    }

    async fn wait_for_completion(&self, execution_id: &str) -> Result<()> {
        let url = format!("{BASE_URL}/execution/{execution_id}/status");
        loop {
            let resp: StatusResponse = self
                .http
                .get(&url)
                .header("X-Dune-API-Key", &self.api_key)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;

            match resp.state.as_str() {
                "QUERY_STATE_COMPLETED" => return Ok(()),
                "QUERY_STATE_FAILED" | "QUERY_STATE_CANCELLED" | "QUERY_STATE_EXPIRED" => {
                    return Err(eyre::eyre!(
                        "dune execution {execution_id} ended in state {}",
                        resp.state
                    ));
                }
                _ => tokio::time::sleep(Duration::from_secs(2)).await,
            }
        }
    }

    async fn fetch_results<T: DeserializeOwned>(&self, execution_id: &str) -> Result<Vec<T>> {
        let url = format!("{BASE_URL}/execution/{execution_id}/results");
        let resp: ResultsResponse<T> = self
            .http
            .get(&url)
            .header("X-Dune-API-Key", &self.api_key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp.result.rows)
    }

    pub async fn run_query<T: DeserializeOwned>(&self, query_id: u64) -> Result<Vec<T>> {
        self.run_query_with_params(query_id, &HashMap::new()).await
    }

    pub async fn run_query_with_params<T: DeserializeOwned>(
        &self,
        query_id: u64,
        params: &HashMap<String, String>,
    ) -> Result<Vec<T>> {
        let execution_id = self.execute(query_id, params).await?;
        self.wait_for_completion(&execution_id).await?;
        self.fetch_results(&execution_id).await
    }
}
