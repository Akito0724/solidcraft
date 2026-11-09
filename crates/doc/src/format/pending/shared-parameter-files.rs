//! Shared parameter files: a design may keep `param_files`, the shared parameter files it reads
//! (each with its path, the parameters it gave and the digest of the bytes last read or written).
//! The parameters themselves stay in `params` as user parameters. An older design has no
//! `param_files` and reads none. The last release warns about the newer format number, opens the
//! design with those parameters as its own and drops the note of where they came from.
