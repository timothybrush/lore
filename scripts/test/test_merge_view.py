# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging
import os

import pytest

from lore import Lore

logger = logging.getLogger(__name__)


@pytest.mark.smoke
def test_merge_restart(new_lore_repo, tmp_path_factory):
    repo: Lore = new_lore_repo()
    source_bin_file1 = "file1.bin"
    source_bin_file2 = "file2.bin"
    source_bin_file3 = "subdir/file3.bin"

    with repo.open_file(source_bin_file1, "w+b") as output_file:
        output_file.write(os.urandom(4096))
    with repo.open_file(source_bin_file2, "w+b") as output_file:
        output_file.write(os.urandom(4096))
    repo.make_dirs(os.path.dirname(source_bin_file3))
    with repo.open_file(source_bin_file3, "w+b") as output_file:
        output_file.write(os.urandom(4096))

    # (source) Stage the files
    repo.stage(scan=True)

    # (source) Commit the files
    repo.commit()

    # (source) Verify the repository
    repo.repository_verify()

    # (source) Push the repository
    repo.branch_push()

    temp_path = tmp_path_factory.mktemp("viewfilter")
    view_filter = os.path.join(temp_path, "view_filter.txt")

    with open(view_filter, "w+") as output_file:
        output_file.writelines(["/file2.bin\n", "/subdir/*\n"])

    # (clone) Clone the repository
    clone = repo.clone(view=view_filter)

    cloned_bin_file1 = "file1.bin"
    cloned_bin_file2 = "file2.bin"
    cloned_bin_file3 = "subdir/file3.bin"

    # (clone) Make a branch
    clone.branch_create("feature-branch")

    # (clone) Switch back to main for now
    clone.branch_switch("main")

    # (clone) Ensure view filter was properly applied in all operations
    assert clone.file_exists(cloned_bin_file1), (
        "File not cloned while it should be for " + cloned_bin_file1
    )
    assert not clone.file_exists(cloned_bin_file2), (
        "File cloned while it should not be for " + cloned_bin_file2
    )
    assert not clone.file_exists(cloned_bin_file3), (
        "File cloned while it should not be for " + cloned_bin_file3
    )

    # (source) Modify all files on source branch
    with repo.open_file(source_bin_file1, "w+b") as output_file:
        output_file.write(os.urandom(4096))
    with repo.open_file(source_bin_file2, "w+b") as output_file:
        output_file.write(os.urandom(4096))
    with repo.open_file(source_bin_file3, "w+b") as output_file:
        output_file.write(os.urandom(4096))

    # (source) Stage the modify
    repo.file_stage(scan=True)

    # (source) Commit the modify
    repo.commit("Modified all files")

    # (source) Push the repository
    repo.branch_push()

    # (clone) Sync on main branch
    clone.revision_sync()

    # (clone) Switch to feature branch
    clone.branch_switch("feature-branch")

    # (clone) Merge in main branch
    clone.branch_merge_start("main")

    # (clone) Ensure view filter was properly applied in all operations
    assert clone.file_exists(cloned_bin_file1), (
        "File not cloned while it should be for " + cloned_bin_file1
    )
    assert not clone.file_exists(cloned_bin_file2), (
        "File cloned while it should not be for " + cloned_bin_file2
    )
    assert not clone.file_exists(cloned_bin_file3), (
        "File cloned while it should not be for " + cloned_bin_file3
    )
