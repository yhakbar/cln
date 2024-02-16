# Cln

A fun little experiment for a git client that clones a git repo, then generates a local directory via links (cln).

It works by:

1. Cloning the metadata of the repository (`.git` directory) into a temporary directory.
2. Parsing that metadata and populating a permanent local store with the repository's contents.
3. Creating a new working directory, entirely linked to the content in the local store.
