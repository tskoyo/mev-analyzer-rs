use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CandidateQueryParams {
    pub start_month: String,
    pub lookback_days: u32,
    pub excluded_symbols: Vec<String>,
    pub allowed_projects: Vec<String>,
    pub min_pool_volume: f64,
    pub max_days_since_trade: u32,
}

impl Default for CandidateQueryParams {
    fn default() -> Self {
        Self {
            start_month: "2026-08-01".to_string(),
            lookback_days: 30,
            excluded_symbols: [
                "WETH", "ETH", "USDC", "USDT", "DAI", "WBTC", "WBNB", "BNB", "WMATIC", "WAVAX",
                "PAXG",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            allowed_projects: [
                "uniswap",
                "sushiswap",
                "pancakeswap",
                "shibaswap",
                "apeswap",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            min_pool_volume: 20_000.0,
            max_days_since_trade: 1,
        }
    }
}

impl CandidateQueryParams {
    pub fn to_query_parameters(&self) -> HashMap<String, String> {
        let mut params = HashMap::new();
        params.insert("start_month".to_string(), self.start_month.clone());
        params.insert("lookback_days".to_string(), self.lookback_days.to_string());
        params.insert(
            "excluded_symbols".to_string(),
            sql_list(&self.excluded_symbols),
        );
        params.insert(
            "allowed_projects".to_string(),
            sql_list(&self.allowed_projects),
        );
        params.insert(
            "min_pool_volume".to_string(),
            self.min_pool_volume.to_string(),
        );
        params.insert(
            "max_days_since_trade".to_string(),
            self.max_days_since_trade.to_string(),
        );
        params
    }
}

fn sql_list(items: &[String]) -> String {
    items
        .iter()
        .map(|s| format!("'{}'", s.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",")
}
