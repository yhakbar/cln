# Cln

A fun little experiment for a tool that clones a git repo, then generates a local directory via links to a persisted content addressable store (CAS).

It works by:

1. Cloning the metadata of the repository (the `.git` directory) into a temporary directory.
2. Parsing that metadata and populating a permanent local store with the repository's contents as read-only files.
3. Creating a new working directory, entirely linked to the content in the local store.

## Installation

You need [rust installed](https://www.rust-lang.org/tools/install) fisrt.

Then run the following in this repo:

```bash
cargo install --path bin
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
cln git@github.com:yhakbar/cln.git
```

## Why?

An optimization that `cln` takes is to run `git ls-remote` to get the object hash that corresponds to the `HEAD` of the remote repository (or another ref if the `-b` flag is used) without downloading any of the objects. This allows `cln` to determine that the current state of the remote repository is already reflected in the local store, and is able to skip the clone step entirely and reconstruct the directory from the local store.

If `HEAD` moves or a different ref is selected, `cln` will still have to perform a bare clone of the repo to update the local store, but it only has to update the local store with the new objects, and not the entire repo.

### Advantages

- **Speed**: Once the local store is populated, `cln` can clone a repo in a fraction of the time it takes to clone a repo with `git clone`, as it only has to make a small network request to determine if the local store is up to date, then create a new working directory linked to the local store.
- **Disk Space**: The local store is a content-addressable store, so if you have multiple clones of the same repo, they will share the same objects, saving disk space. This is especially useful if repos share identical content, such as when using multiple branches of the same repo.

### Disadvantages

- **Read-Only**: The local store is read-only, so you can't make changes to the repo. This is required, as the local store is a content-addressable store, and changing the contents of the store would invalidate the hash of the objects, breaking the ability to link to them reliably. `cln` is also expected to be used in a context where multiple clones of the same repo are made, so it's important that the local store is immutable.
- **Initial Clone**: The initial clone of a repo is going to be slower than a `git clone` because `cln` has to do a lot more work to setup the permanent local store. It's assumed that you'll be cloning the same repo multiple times when using `cln`, however, so the initial clone and store creation time is amortized over multiple clones.

## Benchmarks

The following are very unscientific benchmarks run on my underpowered M1 Air laptop, comparing the time it takes to clone a repo using a minimal `git clone` command vs `cln`.

Note that the initial `cln` clone is skipped due to the `--warmup` flag.

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

## Known Issues

- `cln` doesn't support using a specific commit of a repo. This is because `cln` uses `git ls-remote` to determine the object hash of the `HEAD` of the remote repository, and then uses `git clone --bare --depth 1 --single-branch` to clone the repo. Both of these commands don't support working with a specific commit, so the only feasible way to handle this is to fully clone the repo, then checkout the specific commit. That would defeat the purpose of using `cln` in the first place, so it's not supported.
- Distribution isn't setup. This is a toy project, so I'm not going to spend time setting up a release process for it. If you want to use it, you'll have to clone the repo and build it yourself.
