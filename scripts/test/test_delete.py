# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging
import os
import random
import string

import pytest
from error_types import (
    BranchDivergedError,
)

from lore import Lore

logger = logging.getLogger(__name__)


@pytest.mark.smoke
def test_divergent_delete_resolve_mine_deletes_folder(new_lore_repo):
    repo: Lore = new_lore_repo("test_delete_sync_resove_mine")
    repo.make_dirs("folder")
    with repo.open_file("folder/file.txt", "w+") as output_file:
        output_file.writelines(["A line\n"])

    repo.stage(".", scan=True)
    repo.commit("Initial commit")
    repo.push()

    clone: Lore = repo.clone()
    clone.rmtree("folder")
    clone.stage(".", scan=True)
    clone.commit("Remove folder")
    clone.push()

    with repo.open_file("folder/file.txt", "w+") as output_file:
        output_file.writelines(["An updated line\n"])
    repo.stage(".", scan=True)
    repo.commit("Edit file")
    with pytest.raises(BranchDivergedError):
        repo.push()

    repo.sync()
    repo.branch_merge_resolve_mine("folder/file.txt")
    repo.commit("Resolved conflict")
    repo.push()

    clone.sync()
    assert not clone.file_exists("folder/file.txt"), "Deleted file has returned"
    assert not clone.path_exists("folder"), "Deleted folder has returned"
