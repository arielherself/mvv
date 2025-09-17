# mvv

This project aims to provide an asynchronous and robust `mv` implementation, especially for moving directories between local and remote machines with a poor network connection. The syntax is not compatible with `mv`.

## Features

* Asynchronous moving: `mvv` could execute move tasks concurrently, which opens multiple connections when the source or destination is a mounted net drive.
* Resumable transfer: after the moving process is interrupted (by user, insufficient disk space, unexpected closed connection, etc.), `mvv` continues from the last byte not moved. Although the associated files are still checked byte-by-byte, this is still faster under most conditions.

## Installation

```bash
cargo build --release
sudo install -Dm755 ./target/release/mvv /usr/local/bin/mvv
```

## Usage

There are two main use cases: moving a directory and moving a file.

### Moving a directory

```
mvv <source directory> <destination directory> [max number of concurrent tasks]
```

Please note that `destination directory` should be exactly where you want to move the files to.
For example, `mvv ./src/some_directory ./dst/` will move `./src/some_directory/some_file` to `./dst/some_file`, instead of `./dst/some_directory/some_file`. This is an intended behavior in order to avoid ambiguity.

### Moving a file

```
mvv <source file> <destination file> [max number of concurrent tasks]
```

This usage also follows the rule mentioned above. For example, `mvv ./src/some_file ./dst/` will move `./src/some_file` to `./dst`, instead of `./dst/some_file`.

## Caveats

* It's actually `cp`+`rm`, so the maximum disk usage could be up to twice the total size.
* Mode bits are not kept.
* Symlinks are not currently supported.
