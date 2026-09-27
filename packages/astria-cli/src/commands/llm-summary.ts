/// Shared run-summary lines for the measured LLM work a pipeline run did:
/// token spend (from response usage blocks), cache hits, and the two
/// opt-in enrichment stages. Printed only when there is something to say.
interface PipelineResultSummary {
  semanticCached: number;
  llmInputTokens: number;
  llmOutputTokens: number;
  llmApiCalls: number;
  communitiesLabeled: number;
  communitiesReused: number;
  communitiesFailed: number;
  deepLinks: number;
}

export function printLlmSummary(result: PipelineResultSummary): void {
  if (result.llmApiCalls > 0) {
    const cached = result.semanticCached > 0 ? `, ${result.semanticCached} files from cache` : '';
    console.log(
      `LLM usage: ${result.llmApiCalls} API calls, ${result.llmInputTokens} in / ${result.llmOutputTokens} out tokens${cached}`,
    );
  }
  if (result.communitiesLabeled >= 0) {
    const reused = result.communitiesReused > 0 ? `, ${result.communitiesReused} unchanged` : '';
    const failed = result.communitiesFailed > 0 ? `, ${result.communitiesFailed} failed (kept hub names)` : '';
    console.log(`Communities labeled: ${result.communitiesLabeled}${reused}${failed} (--label-communities)`);
  }
  if (result.deepLinks >= 0) {
    console.log(`Deep concept links: ${result.deepLinks} (--deep)`);
  }
}
