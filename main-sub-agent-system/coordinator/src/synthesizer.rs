use agent_teams_core::boxed_agent::AgentOutput;

/// Synthesizer: combines results from multiple agents
pub struct Synthesizer;

impl Synthesizer {
    pub fn new() -> Self {
        Self
    }

    /// Find the main narrative from results
    pub fn extract_narrative(&self, results: &[(String, AgentOutput)]) -> String {
        // Prefer main_agent result, then highest quality
        if let Some((_, resp)) = results.iter().find(|(id, _)| id == "main_agent") {
            if !resp.content.is_empty() {
                return resp.content.clone();
            }
        }

        results
            .iter()
            .filter(|(_, r)| !r.content.is_empty())
            .max_by(|a, b| {
                a.1.quality
                    .partial_cmp(&b.1.quality)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(_, r)| r.content.clone())
            .unwrap_or_default()
    }
}

impl Default for Synthesizer {
    fn default() -> Self {
        Self::new()
    }
}
