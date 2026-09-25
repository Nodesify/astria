"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.migrateCommand = migrateCommand;
const fs_1 = require("fs");
const path_1 = require("path");
function homeDir() {
    return process.env.USERPROFILE || process.env.HOME || '.';
}
function moveOrReport(from, to, label) {
    if (!(0, fs_1.existsSync)(from))
        return false;
    if ((0, fs_1.existsSync)(to)) {
        console.log(`[migrate] ${label}: ${to} already exists — left ${from} untouched. Remove it manually once verified.`);
        return false;
    }
    (0, fs_1.renameSync)(from, to);
    console.log(`[migrate] ${label}: ${from} -> ${to}`);
    return true;
}
/// One-time migration from the pre-1.0 nodesify-graphify layout. 1.0 reads
/// only `.astria/`, so this is the escape hatch for existing graphs: the
/// data folder, ignore file, and home-dir global store are renamed in place.
function migrateCommand(opts) {
    const root = opts.graph;
    if (!(0, fs_1.existsSync)(root)) {
        throw new Error(`path not found: ${root}`);
    }
    let moved = 0;
    moved += Number(moveOrReport((0, path_1.join)(root, '.graphify'), (0, path_1.join)(root, '.astria'), 'data folder'));
    moved += Number(moveOrReport((0, path_1.join)(root, '.graphifyignore'), (0, path_1.join)(root, '.astriaignore'), 'ignore file'));
    const globalFrom = (0, path_1.join)(homeDir(), '.nodesify-graphify', 'global.db');
    const globalTo = (0, path_1.join)(homeDir(), '.astria', 'global.db');
    if ((0, fs_1.existsSync)(globalFrom)) {
        if ((0, fs_1.existsSync)(globalTo)) {
            console.log(`[migrate] global store: ${globalTo} already exists — left ${globalFrom} untouched.`);
        }
        else {
            (0, fs_1.mkdirSync)((0, path_1.join)(homeDir(), '.astria'), { recursive: true });
            (0, fs_1.renameSync)(globalFrom, globalTo);
            console.log(`[migrate] global store: ${globalFrom} -> ${globalTo}`);
            moved += 1;
        }
    }
    if (moved === 0) {
        console.log('[migrate] nothing to migrate (no pre-1.0 layout found).');
    }
    else {
        console.log(`[migrate] done — ${moved} item(s) moved. Run \`astria update ${root}\` to refresh.`);
    }
}
//# sourceMappingURL=migrate.js.map