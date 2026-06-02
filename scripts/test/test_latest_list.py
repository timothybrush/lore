# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging

import pytest

from lore import Lore

logger = logging.getLogger(__name__)


@pytest.mark.smoke
def test_latest_list(new_lore_repo):
    repo: Lore = new_lore_repo("LatestList")

    # Generate some files
    text_file = "text-File.txt"

    for i in range(0, 3):
        with repo.open_file(text_file, "w+") as output_file:
            output_file.writelines(
                ["One line\n", "Another line\n", "Third line\n {}".format(i)]
            )

        repo.stage(scan=True)
        repo.commit()
        repo.push()

    result = repo.branch_latest_list()
    lines = result.split("\n")
    hash_lines = []
    for line in lines:
        if line.strip():
            hash_lines.append(line)

    assert len(hash_lines) == 3, f"Expected 3 revisions but got ${len(lines)}"
