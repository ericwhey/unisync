# unisync

unisync is designed as a replacement for the unison file/dir synchronization tool.  

It's use case is for interactive comparison and synchronization of large file collections.

*Git* is not directly useful because it tends to keep previous versions and for large files that is inefficient.

*SyncThing* or similar tools tend to copy files over old files without asking which would be bad if your files were suddenly encrypted by ransomware.

*rsync* is not particularly bidirectional takes some mental gymnastics that some people wouldn't 

unisync works in three phases.

### Phase 1 - Scanning 

The first phase is scanning which compares the current state of a repository to the previous state of a repository.  The state is stored in a repository registry file in:

```
repository/.unisync/last.txt
```

If it is a new repository then the repository registry is built for the first time and all files are marked as *NEW*.

If it is an existing repository then any changed files are marked as *MODIFIED* and any deleted files are marked as *DELETED*.

### Phase 2 - Comparison

The second phase is comparison which compares the registry of two repositories and creates a differences list that keeps track of changes in either repository.

### Phase 3 - Synchronization

An interactive process of dealing with differences in the two repositories.  For each difference a keypress indicates what will action will be performed:

*'/'* skips the difference

*RETURN* accepts the suggested action for the difference

*'f'* does the forward action from repository 1 to repository 2

*'r'* does the reverse action from repository 2 to repository 1

*'\*'* skips the difference and all future differences of that type

Much credit goes to unison for filling an important need.  However the use of non-standard build tools and lack of ongoing improvements presents some issues.

This tool is still under development and is still in alpha but will be ready for beta soon.  Suggestions and changes are welcome as the development of this tool has so far been for learning Rust and to specifically backup a media collection.  Additional ideas for functionality and help cleaning up code are specifically requested.

## Build

```sh
git clone https://github.com/ericwhey/unisync.git

cd unisync

cargo build --release --workspace
```

## Installation

First make sure that you have a bin directory in your home directory and it is in your path.

```sh
cp target/release/unisync ~/bin/unisync
```

## Usage

```sh
unisync repository1 [repository2] [--temp temp_dir] [--noperms] [--notimes] [--nocompress]
```

### repository1
If only one repository is provided then only the scanning phase is performed.  

### repository2

If two respositories are provided, then both are scanned and a comparison is performed.  This is followed by an interactive synchronization process.

In the near future repositories available over SSH will be supported.

### --temp 

The temp directory changes where the repository registry is built during the scanning phase.  Once the scanning phase is complete, the registry is moved into the repository.  This is useful for situations where a repository might be on spinning rust and you want your temp file built on solid state drives.

### --noperms

Instructs the comparison phase to ignore any changes in only permissions.  This seems to be necessary on certain file systems like NTFS where permissions are not stored.

### --notimes

Instructs the comparison phase to ignore any changes in only modification timestamps.

### --nocompress

Instructs the comparison phase not to compress missing groups of files into missing directories where possible.  

## Todo

The following are specific todo items:
- Make repositories 

