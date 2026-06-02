# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging
import os
import uuid

import pytest

from lore import Lore

logger = logging.getLogger(__name__)


@pytest.mark.smoke
def test_filter(new_lore_repo):
    repo_id = uuid.uuid4().hex
    view_file = "view.txt"
    files_to_exclude = [
        "pathC/subB/file.txt",
        "pathI/file.txt~theirs",
        "pathJ/file.txt~base",
        "pathK/file.txt.~loretemp",
        "root.bin~theirs",
        "root.bin~base",
        "root.bin.~loretemp",
    ]

    repo: Lore = new_lore_repo("Filter", repo_id=repo_id)

    paths = [
        "pathA/file.txt",
        "pathB/subA/file.txt",
        "pathD/file.txt",
        "pathE/subA/subB/file.txt",
    ]
    paths.extend(files_to_exclude)

    for path in paths:
        repo.make_dirs(os.path.dirname(path))
        with repo.open_file(path, "w+b") as output_file:
            output_file.write(os.urandom(1024))

    # Generate view filter file
    with repo.open_file(view_file, "w+") as output_file:
        output_file.writelines("/pathC/subB/*\n")

    # Status the repository
    output = repo.repository_status(unstaged=True)

    assert not "~theirs" in output, f"Expected `~theirs` file to be ignored."
    assert not "~base" in output, f"Expected `~base` file to be ignored."
    assert not "~loretemp" in output, f"Expected `~loretemp` file to be ignored."

    repo.stage(scan=True)
    repo.commit()
    repo.branch_push()

    # Clone the repository
    clone = repo.clone(view=view_file)

    for file in files_to_exclude:
        assert not clone.file_exists(file), f"Expected `{file}` not to exist."

    # Modified files in source repo
    for path in paths:
        with repo.open_file(path, "w+b") as output_file:
            output_file.write(os.urandom(256))

    repo.stage(scan=True)
    repo.commit()
    repo.branch_push()

    # Sync
    clone.sync()

    for file in files_to_exclude:
        assert not clone.file_exists(file), f"Expected `{file}` not to exist."
