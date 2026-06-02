# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging
import os

import pytest

from lore import Lore

logger = logging.getLogger(__name__)


@pytest.mark.smoke
def test_lock_branch(new_lore_repo):
    repo: Lore = new_lore_repo("LockBranch")
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

    with repo.open_file(repo.ignore_file(), "w") as output_file:
        output_file.writelines("ignore.txt")

    repo.stage(scan=True)
    repo.commit()
    repo.push()

    repo.branch_create("dev")

    # Lock acquire @ main
    assert (
        len(
            repo.lock_acquire(
                [
                    "ignore.txt",
                    "subdir_a/file_a.txt",
                    "subdir_b/file_b.txt",
                    "subdir_c/subdir/file_c.txt",
                ],
                "main",
            ).acquired
        )
        == 3
    )

    # Lock status @ dev - no action
    assert (
        len(
            repo.lock_status(
                [
                    "ignore.txt",
                    "subdir_a/file_a.txt",
                    "subdir_b/file_b.txt",
                    "subdir_c/subdir/file_c.txt",
                ]
            )
        )
        == 0
    )

    # Lock status @ main
    assert (
        len(
            repo.lock_status(
                [
                    "ignore.txt",
                    "subdir_a/file_a.txt",
                    "subdir_b/file_b.txt",
                    "subdir_c/subdir/file_c.txt",
                ],
                "main",
            )
        )
        == 3
    )

    # Lock release @ dev - locks do not exist
    assert (
        len(
            repo.lock_release(
                [
                    "ignore.txt",
                    "subdir_a/file_a.txt",
                    "subdir_b/file_b.txt",
                    "subdir_c/subdir/file_c.txt",
                ]
            ).released
        )
        == 0
    )

    # Lock release @ main
    assert (
        len(
            repo.lock_release(
                [
                    "ignore.txt",
                    "subdir_a/file_a.txt",
                    "subdir_b/file_b.txt",
                    "subdir_c/subdir/file_c.txt",
                ],
                "main",
            ).released
        )
        == 3
    )
