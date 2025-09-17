use std::sync::Arc;
use std::{env::args, path::Path};

use anyhow::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _};
use tokio::sync::Semaphore;
use walkdir::WalkDir;

async fn move_file(
    src_path: impl AsRef<Path>,
    dest_path: impl AsRef<Path>,
    buf_size: usize,
    multi_progress: Arc<MultiProgress>,
    permits: Arc<Semaphore>,
) -> Result<()> {
    let _guard = permits.acquire().await?;

    let progress_style = ProgressStyle::with_template(
        "[{binary_bytes_per_sec}] {wide_bar} {msg:70!} {bytes}/{total_bytes}",
    )?;

    if !std::fs::exists(&src_path)? {
        return Err(anyhow::anyhow!(
            "source file \"{}\" does not exist",
            src_path.as_ref().display()
        ));
    }

    if let Some(dest_parent_path) = dest_path.as_ref().parent() {
        tokio::fs::create_dir_all(dest_parent_path).await?;
    }

    let (init_offset, mut progress_bar) = if std::fs::exists(&dest_path)? {
        let buf_size = buf_size / 2;
        let mut src_buf = vec![0; buf_size];
        let mut dest_buf = vec![0; buf_size];

        let mut src_file = tokio::fs::File::open(&src_path).await?;
        let src_size = src_file.metadata().await?.len() as usize;

        let mut dest_file = tokio::fs::File::open(&dest_path).await?;
        let dest_size = dest_file.metadata().await?.len() as usize;

        let min_size = src_size.min(dest_size);
        let mut read = 0;

        let progress_bar = multi_progress.add(ProgressBar::new(src_size as u64));
        progress_bar.set_style(progress_style.clone());
        progress_bar.set_message(format!(
            "checking {}",
            src_path.as_ref().file_name().unwrap().display()
        ));

        while read < min_size {
            let read_max_size = src_buf.len().min(min_size - read);

            let curr_src_read = src_file.read(&mut src_buf[..read_max_size]).await?;
            let curr_dest_read = dest_file.read(&mut dest_buf[..read_max_size]).await?;

            if curr_src_read < curr_dest_read {
                src_file
                    .read_exact(&mut src_buf[curr_src_read..curr_dest_read])
                    .await?;
            } else if curr_src_read > curr_dest_read {
                dest_file
                    .read_exact(&mut dest_buf[curr_dest_read..curr_src_read])
                    .await?;
            }

            let curr_read = curr_src_read.max(curr_dest_read);

            for (&x, &y) in src_buf[..curr_read]
                .iter()
                .zip(dest_buf[..curr_read].iter())
            {
                if x != y {
                    break;
                }
                read += 1;
            }
        }

        (read as u64, Some(progress_bar))
    } else {
        (0u64, None)
    };

    let mut src_file = tokio::fs::File::open(&src_path).await?;
    let src_size = src_file.metadata().await?.len() as usize;
    src_file.seek(std::io::SeekFrom::Start(init_offset)).await?;

    let mut dest_file = tokio::fs::File::options()
        .create(true)
        .write(true)
        .open(&dest_path)
        .await?;
    dest_file
        .seek(std::io::SeekFrom::Start(init_offset))
        .await?;

    if progress_bar.is_none() {
        progress_bar = Some(multi_progress.add(ProgressBar::new(src_size as u64)));
    }
    let progress_bar = progress_bar.unwrap();

    progress_bar.set_style(progress_style);
    progress_bar.set_position(init_offset);
    progress_bar.set_message(format!(
        "copying {:?}",
        src_path.as_ref().file_name().unwrap().display()
    ));

    tokio::io::copy(&mut src_file, &mut progress_bar.wrap_async_read(dest_file)).await?;
    drop(src_file);

    tokio::fs::remove_file(src_path).await?;

    progress_bar.finish_with_message("complete");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = args().collect::<Vec<_>>();
    if args.len() != 3 && args.len() != 4 {
        return Err(anyhow::anyhow!(
            "incorrect syntax\nusage: {} <source> <destination> [paralleled-jobs]",
            args[0]
        ));
    }

    let paralleled_jobs = if args.len() == 4 {
        args[3].parse().unwrap()
    } else {
        4
    };

    let permits = Arc::new(Semaphore::new(paralleled_jobs));

    let src_path = Path::new(&args[1]);
    let dest_path = Path::new(&args[2]);

    let src_is_file = src_path.is_file();

    let multi_progress = Arc::new(MultiProgress::new());

    let mut tasks = vec![];

    for entry in WalkDir::new(src_path) {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            multi_progress.println(format!(
                "warning: symlink \"{}\" is skipped",
                entry.path().display()
            ))?;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let rel_path = Path::strip_prefix(entry.path(), src_path)?;
        let (src_path, dest_path) = if src_is_file {
            (src_path.to_path_buf(), dest_path.to_path_buf())
        } else {
            (
                Path::join(src_path, rel_path),
                Path::join(dest_path, rel_path),
            )
        };
        if entry.file_type().is_file() {
            tasks.push((
                src_path.clone(),
                tokio::spawn(move_file(
                    src_path,
                    dest_path,
                    10_000_000,
                    Arc::clone(&multi_progress),
                    Arc::clone(&permits),
                )),
            ));
        }
    }

    let mut incomplete = false;

    for (path, task) in tasks {
        if let Err(e) = task.await? {
            incomplete = true;
            multi_progress.println(format!("error when moving \"{}\": {}", path.display(), e))?;
        }
    }

    if incomplete {
        return Err(anyhow::anyhow!(
            "an error occurred when moving one or more files"
        ));
    }

    tokio::fs::remove_dir_all(src_path).await?;

    multi_progress.println("move complete")?;
    Ok(())
}
