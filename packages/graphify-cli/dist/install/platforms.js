"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.PLATFORM_NAMES = exports.PLATFORMS = void 0;
const path = __importStar(require("path"));
const os = __importStar(require("os"));
exports.PLATFORMS = {
    claude: {
        skillFile: 'skill.md',
        skillDst: path.join('.claude', 'skills', 'graphify', 'SKILL.md'),
        claudeMd: true,
        agentsMd: false,
        geminiMd: false,
        settingsHook: 'claude',
        mcp: 'claude',
    },
    codex: {
        skillFile: 'skill-codex.md',
        skillDst: path.join('.agents', 'skills', 'graphify', 'SKILL.md'),
        claudeMd: false,
        agentsMd: true,
        geminiMd: false,
        settingsHook: 'codex',
    },
    gemini: {
        skillFile: 'skill-gemini.md',
        skillDst: os.platform() === 'win32'
            ? path.join('.agents', 'skills', 'graphify', 'SKILL.md')
            : path.join('.gemini', 'skills', 'graphify', 'SKILL.md'),
        claudeMd: false,
        agentsMd: false,
        geminiMd: true,
        settingsHook: 'gemini',
        mcp: 'gemini',
    },
    opencode: {
        skillFile: 'skill-opencode.md',
        skillDst: path.join('.config', 'opencode', 'skills', 'graphify', 'SKILL.md'),
        claudeMd: false,
        agentsMd: true,
        geminiMd: false,
        settingsHook: 'opencode',
    },
    cursor: {
        skillFile: '',
        skillDst: '',
        claudeMd: false,
        agentsMd: false,
        geminiMd: false,
        settingsHook: 'none',
        mcp: 'cursor',
    },
    kiro: {
        skillFile: 'skill.md',
        skillDst: path.join('.kiro', 'skills', 'graphify', 'SKILL.md'),
        claudeMd: false,
        agentsMd: false,
        geminiMd: false,
        settingsHook: 'none',
    },
    aider: {
        skillFile: 'skill-aider.md',
        skillDst: path.join('.aider', 'skills', 'graphify', 'SKILL.md'),
        claudeMd: false,
        agentsMd: true,
        geminiMd: false,
        settingsHook: 'none',
    },
    copilot: {
        skillFile: 'skill-copilot.md',
        skillDst: path.join('.github', 'skills', 'graphify', 'SKILL.md'),
        claudeMd: false,
        agentsMd: true,
        geminiMd: false,
        settingsHook: 'none',
    },
    trae: {
        skillFile: 'skill-trae.md',
        skillDst: path.join('.trae', 'skills', 'graphify', 'SKILL.md'),
        claudeMd: false,
        agentsMd: true,
        geminiMd: false,
        settingsHook: 'none',
    },
    zcode: {
        // skill-codex.md is CLI-oriented, which is what ZCode sessions use
        // unless the graphify MCP server is registered by the mcp flag below.
        skillFile: 'skill-codex.md',
        skillDst: path.join('.zcode', 'skills', 'graphify', 'SKILL.md'),
        claudeMd: false,
        agentsMd: true,
        geminiMd: false,
        settingsHook: 'none',
        mcp: 'zcode',
    },
};
exports.PLATFORM_NAMES = Object.keys(exports.PLATFORMS);
//# sourceMappingURL=platforms.js.map