# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging
import os

import pytest

from error_types import LockInvalidPath
from lore import Lore

logger = logging.getLogger(__name__)


@pytest.mark.smoke
def test_lock_release(new_lore_repo):
    repo: Lore = new_lore_repo("LockRelease")
    paths = [
        "ignore.txt",
        "subdir_a/file_a.txt",
        "subdir_b/file_b.txt",
        "subdir_c/subdir/file_c.txt",
    ]
    for path in paths:
        repo.make_dirs(os.path.dirname(path))
        with repo.open_file(path, "w+b") as output_file:
            output_file.write(os.urandom(1024))

    with open(os.path.join(repo.path, repo.ignore_file()), "w") as output_file:
        output_file.writelines("ignore.txt")

    repo.stage(scan=True)
    repo.commit()
    repo.push()

    # Lock acquire
    assert (
        len(
            repo.lock_acquire(
                ["subdir_a/file_a.txt", "subdir_b/file_b.txt", "ignore.txt"]
            ).acquired
        )
        == 2
    )

    # Remove file from repository, commit and push
    repo.remove_file("subdir_b/file_b.txt")

    repo.stage(scan=True)
    repo.commit()
    repo.push()

    # Lock release
    assert len(repo.lock_release(["subdir_a/file_a.txt", "ignore.txt"]).released) == 1

    # Lock release on file removed from repository
    with pytest.raises(LockInvalidPath):
        repo.lock_release("subdir_b/file_b.txt")

    assert len(repo.lock_release("subdir_b/file_b.txt", force=True).released) == 1
