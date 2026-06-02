# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging
import os
import random
import string

import pytest

from lore import Lore

logger = logging.getLogger(__name__)


@pytest.mark.smoke
def test_deduplicate(new_lore_repo):
    # Create source repository
    repo: Lore = new_lore_repo("DeduplicationFirst")
    test_file = "test-file.txt"

    with repo.open_file(test_file, "w+") as output_file:
        output_file.writelines(["One line\n", "Another line\n", "Third line\n"])
        output_file.writelines(["One line\n", "Another line\n", "Third line\n"])
        output_file.writelines(["One line\n", "Another line\n", "Third line\n"])
        output_file.writelines(["One line\n", "Another line\n", "Third line\n"])
        output_file.writelines(["One line\n", "Another line\n", "Third line\n"])

        alphabet = string.ascii_letters + string.digits + " .,;:!"
        for _ in range(100):
            line = "".join(random.choices(alphabet, k=100))
            output_file.writelines([line])

    repo.stage(scan=True)
    repo.commit("Test commit 1")
    repo.push()

    # Create second repository
    repo2: Lore = new_lore_repo("DeduplicationSecond")

    # Force a different representation of the data
    target_file = "first-file.txt"
    repo.copy2(test_file, target_file, to_repo=repo2)
    repo2.environment_vars["LORE_COMPRESSION_LEVEL"] = "4"

    repo2.stage(scan=True)
    repo2.commit("Test commit 2")
    repo2.push()

    # Force a different representation of the data
    target_file = "second-file.txt"
    repo.copy2(test_file, target_file, to_repo=repo2)
    repo2.environment_vars["LORE_COMPRESSION_LEVEL"] = "1"

    repo2.stage(scan=True)
    repo2.commit("Test commit 3", offline=True)
    repo2.push()

    # Clone the second repository
    repo_cloned = repo2.clone(name="DeduplicationSecond-Clone")

    first_file = "first-file.txt"
    second_file = "second-file.txt"
    assert repo.compare_file(repo_cloned, test_file, first_file), (
        "File comparison after clone failed"
    )
    assert repo.compare_file(repo_cloned, test_file, second_file), (
        "File comparison after clone failed"
    )
