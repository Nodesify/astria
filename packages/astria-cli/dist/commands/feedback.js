"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.saveResultCommand = saveResultCommand;
exports.reflectCommand = reflectCommand;
const native_1 = require("../native");
async function saveResultCommand(question, opts) {
    try {
        let answer = opts.answer;
        if (!answer && opts.answerFile) {
            answer = require('fs').readFileSync(opts.answerFile, 'utf-8');
        }
        if (!answer) {
            console.error('Error: provide --answer or --answer-file');
            process.exitCode = 1;
            return;
        }
        const sourceNodes = (opts.nodes || '')
            .split(',')
            .map((s) => s.trim())
            .filter(Boolean);
        const saved = (0, native_1.saveQueryResult)(opts.graph, question, answer, opts.outcome, opts.correction, sourceNodes.length > 0 ? sourceNodes : undefined);
        console.log(`Memory saved: ${saved.memoryPath}`);
        console.log(`Graph node: ${saved.nodeId}`);
        console.log('Run `astria update .` to re-embed and re-cluster.');
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
async function reflectCommand(opts) {
    try {
        const lessons = (0, native_1.reflectGraph)(opts.graph);
        console.log(lessons);
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
//# sourceMappingURL=feedback.js.map