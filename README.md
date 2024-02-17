# Cln

A fun little experiment for a tool that clones a git repo, then generates a local directory via links to a persisted content addressable store (CAS).

It works by:

1. Cloning the metadata of the repository (the `.git` directory) into a temporary directory.
2. Parsing that metadata and populating a permanent local store with the repository's contents as read-only files.
3. Creating a new working directory, entirely linked to the content in the local store.

## Installation

```bash
cargo install --path .
```

## Usage

Under the hood, `cln` invokes `git` directly for any interaction with a git repo/remote. As a consequence, you can use `cln` as an alternative for where you would use `git clone` if you only cared about reading the contents of a repo at a specific point in time.

e.g.

```bash
$ cln git@github.com:yhakbar/cln.git
Cloning into bare repository '/var/folders/x1/psy2lqh14v7fzpd8v3kgnp840000gn/T/cln.Iyh9s29qc6sN'...
remote: Enumerating objects: 9, done.
remote: Counting objects: 100% (9/9), done.
remote: Compressing objects: 100% (6/6), done.
remote: Total 9 (delta 0), reused 5 (delta 0), pack-reused 0
Receiving objects: 100% (9/9), 6.65 KiB | 6.65 MiB/s, done.
```

Does the following:

1. Run `git clone --bare --depth 1 --single-branch git@github.com:yhakbar/cln.git` into a temporary directory.
2. Use `git` commands to populate a permanent local store located at `~/.cln-store` with read-only contents of the repo.
3. Create a new working directory with content linked to the local store.

Removing the repo, then re-cloning it should be much faster:

```bash
$ cln git@github.com:yhakbar/cln.git
```

## Why?

An optimization that `cln` takes is to run `git ls-remote` to get the object hash that corresponds to the `HEAD` of the remote repository (or another ref if the `-b` flag is used) without downloading any of the objects. This allows `cln` to determine that the current state of the remote repository is already reflected in the local store, and is able to skip the clone step entirely and reconstruct the directory from the local store.

If `HEAD` moves or a different ref is selected, `cln` will still have to perform a bare clone of the repo to update the local store, but it only has to update the local store with the new objects, and not the entire repo.

## Benchmarks

The following are very unscientific benchmarks run on my underpowered M1 Air laptop, comparing the time it takes to clone a repo using a minimal `git clone` command vs `cln`.

Note that the initial `cln` clone is going to be a lot slower than a `git clone` because it has to do a lot more work to setup the permanent local store. It's assumed that you'll be cloning the same repo multiple times when using `cln`, however, so the initial clone and store creation time is amortized over multiple clones.

### Small Repo

```bash
$ gh api -q '.size' 'repos/yhakbar/cln'
13
```

```bash
$ hyperfine --warmup 10 --runs 10 --shell bash 'tmp="$(mktemp -d)" && git clone --depth 1 --single-branch git@github.com:yhakbar/cln.git "$tmp" && rm -rf "$tmp"'
Benchmark 1: tmp="$(mktemp -d)" && git clone --depth 1 --single-branch git@github.com:yhakbar/cln.git "$tmp" && rm -rf "$tmp"
  Time (mean ± σ):     500.6 ms ±  12.0 ms    [User: 2.2 ms, System: 6.7 ms]
  Range (min … max):   471.4 ms … 513.6 ms    10 runs
```

```bash
$ hyperfine --warmup 10 --runs 10 --shell bash 'tmp="$(mktemp -d)" && cln git@github.com:yhakbar/cln.git "$tmp" && rm -rf "$tmp"'
Benchmark 1: tmp="$(mktemp -d)" && cln git@github.com:yhakbar/cln.git "$tmp" && rm -rf "$tmp"
  Time (mean ± σ):     393.8 ms ±   7.5 ms    [User: 3.3 ms, System: 5.1 ms]
  Range (min … max):   379.6 ms … 405.8 ms    10 runs
```

### Medium Repo

```bash
$ gh api -q '.size' 'repos/lua/lua'
10597
```

```bash
$ hyperfine --warmup 10 --runs 10 --shell bash 'tmp="$(mktemp -d)" && git clone --depth 1 --single-branch https://github.com/lua/lua "$tmp" && rm -rf "$tmp"'
Benchmark 1: tmp="$(mktemp -d)" && git clone --depth 1 --single-branch https://github.com/lua/lua "$tmp" && rm -rf "$tmp"
  Time (mean ± σ):     388.5 ms ± 147.6 ms    [User: 0.9 ms, System: 12.1 ms]
  Range (min … max):   312.9 ms … 804.2 ms    10 runs
```

```bash
$ hyperfine --warmup 10 --runs 10 --shell bash 'tmp="$(mktemp -d)" && cln https://github.com/lua/lua "$tmp" && rm -rf "$tmp"'
Benchmark 1: tmp="$(mktemp -d)" && cln https://github.com/lua/lua "$tmp" && rm -rf "$tmp"
  Time (mean ± σ):     145.1 ms ±  51.4 ms    [User: 0.7 ms, System: 6.9 ms]
  Range (min … max):   123.7 ms … 290.7 ms    10 runs
```

### Big Repo

```bash
$ gh api -q '.size' 'repos/torvalds/linux'
4910356
```

```bash
$ hyperfine --warmup 10 --runs 10 --shell bash 'tmp="$(mktemp -d)" && git clone --depth 1 --single-branch https://github.com/torvalds/linux "$tmp" && rm -rf "$tmp"'
Benchmark 1: tmp="$(mktemp -d)" && git clone --depth 1 --single-branch https://github.com/torvalds/linux "$tmp" && rm -rf "$tmp"
  Time (mean ± σ):     37.109 s ±  0.387 s    [User: 0.032 s, System: 3.517 s]
  Range (min … max):   36.682 s … 37.840 s    10 runs
```

```bash
$ hyperfine --warmup 10 --runs 10 --shell bash 'tmp="$(mktemp -d)" && cln https://github.com/torvalds/linux "$tmp" && rm -rf "$tmp"'
Benchmark 1: tmp="$(mktemp -d)" && cln https://github.com/torvalds/linux "$tmp" && rm -rf "$tmp"
  Time (mean ± σ):     21.006 s ±  0.128 s    [User: 0.034 s, System: 4.358 s]
  Range (min … max):   20.840 s … 21.249 s    10 runs
```
