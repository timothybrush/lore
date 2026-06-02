# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging
import os

import pytest

from lore import Lore

logger = logging.getLogger(__name__)


@pytest.mark.smoke
def test_p4shelve(new_lore_repo):
    repo: Lore = new_lore_repo()
    # Generate some files
    text_file = "text-File.txt"
    binary_file = "binary-File.bin"

    with repo.open_file(text_file, "w+") as output_file:
        output_file.writelines(["One line\n", "Another line\n", "Third line\n"])

    with repo.open_file(binary_file, "w+b") as output_file:
        output_file.write(os.urandom(4096))

    # Stage the files
    repo.stage(scan=True)

    # Set p4-changelist revision metadata
    p4_changelist = "123456789"
    repo.revision_metadata_set(["p4-changelist", p4_changelist])

    repo.commit("Initial snapshot - add text & binary files")
    repo.branch_push()

    feature_branch = "feature-branch"
    repo.branch_create(feature_branch)

    # Delete a file
    repo.remove_file(binary_file)

    # Modify a file
    with repo.open_file(text_file, "w+") as output_file:
        output_file.writelines(
            ["One line\n", "Another line\n", "Third line\n", "Fourth line\n"]
        )

    # Add a file
    text_file_2 = "text-File-2.txt"
    with repo.open_file(text_file_2, "w+") as output_file:
        output_file.writelines(["One line\n", "Another line\n"])

    # Add a directory
    subdir_name = "subdir"
    repo.make_dirs(subdir_name)

    repo.stage(scan=True)
    repo.commit("Feature branch - add, delete, modify")
    repo.branch_push()

    # Test the P4Shelve Lore CLI integration

    # Clone Lore repository
    clone = repo.clone()
    clone.branch_switch(feature_branch)

    # Get P4 base revision and the P4 changelist metadata from the revision
    p4_base_revision = clone.revision_find_metadata("p4-changelist")
    clone_p4_changelist = clone.revision_info(p4_base_revision)
    assert int(clone_p4_changelist.changelist) == int(p4_changelist), (
        "Unexpected p4 changelist"
    )

    # Get revision diff against the branching point revision
    diff = clone.revision_diff(p4_base_revision).splitlines()
    assert len(diff) == 4, "Unexpected diff length"
    assert "D binary-File.bin" in diff
    assert "M text-File.txt" in diff
    assert "A text-File-2.txt" in diff
    assert f"A {subdir_name}" in diff

    # Get diff entry file types
    output = clone.file_info("binary-File.bin", revision=p4_base_revision)
    assert output[0].type == "file", "Could not determine the file type of deleted file"
    output = clone.file_info("text-File-2.txt")
    assert output[0].type == "file", (
        "Could not determine the file type of existing file"
    )
    output = clone.file_info(subdir_name)
    assert output[0].type == "dir", (
        "Could not determine the file type of existing directory"
    )
