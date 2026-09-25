// astria OpenCode plugin
import { existsSync } from "fs";
import { join } from "path";

export const AstriaPlugin = async ({ directory }) => {
  const reminded = new Set();
  return {
    "tool.execute.before": async (input, output) => {
      if (reminded.has(input.tool)) return;
      if (!["view", "grep", "glob", "ls", "bash"].includes(input.tool)) return;
      if (!existsSync(join(directory, ".astria", "graph.json"))) return;
      if (input.tool === "bash") {
        output.args.command =
          'echo "[astria] Knowledge graph available. MUST read .astria/graph_report.md before searching raw files. Use astria query instead of grep for architecture questions." && ' +
          output.args.command;
      } else {
        output.error = new Error(
          "[astria] Knowledge graph available. MUST read .astria/graph_report.md before searching raw files. Use astria query instead of grep for architecture questions."
        );
      }
      reminded.add(input.tool);
    },
  };
};
