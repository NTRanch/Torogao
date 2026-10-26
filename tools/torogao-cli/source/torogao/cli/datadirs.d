/*
    Torogao data directory resolution

    Copyright © 2026, Nice Thoughts Ranch
    Distributed under the 2-Clause BSD License, see LICENSE file.

    Templates and contracts are data files that ship in the
    repository (templates/, contracts/) and are located
    relative to the torogao-cli executable, so the same
    resolution works from a source checkout and from a
    staged tree with the same layout. An environment
    variable (TOROGAO_TEMPLATES_DIR / TOROGAO_CONTRACTS_DIR)
    overrides the search for tests and pack layouts.

    Search order (first directory containing the file wins):
      1. $<envVar>, when set
      2. <exe dir>/../../../<leaf>   (the repository root,
         three levels above tools/torogao-cli/out/)
      3. <exe dir>/../<leaf>
      4. ./<leaf>                    (current directory)
*/
module torogao.cli.datadirs;

import std.file : exists, isDir;
import std.path : buildPath, dirName;
import std.process : environment;
import std.file : thisExePath;

/// Candidate directories for a data leaf, in search order.
string[] candidateDataDirs(string envVar, string leaf) {
    string[] dirs;
    if (auto e = environment.get(envVar))
        if (e.length > 0) dirs ~= e;
    string exeDir = dirName(thisExePath());
    dirs ~= buildPath(exeDir, "..", "..", "..", leaf);
    dirs ~= buildPath(exeDir, "..", leaf);
    dirs ~= buildPath(".", leaf);
    return dirs;
}

/// First candidate directory that contains `fileName`, or "".
string findDataFile(string envVar, string leaf, string fileName) {
    foreach (dir; candidateDataDirs(envVar, leaf)) {
        string path = buildPath(dir, fileName);
        if (exists(path) && !isDir(path)) return path;
    }
    return "";
}

/// All existing candidate directories for a leaf (for listing).
string[] existingDataDirs(string envVar, string leaf) {
    string[] dirs;
    foreach (dir; candidateDataDirs(envVar, leaf))
        if (exists(dir) && isDir(dir)) dirs ~= dir;
    return dirs;
}
