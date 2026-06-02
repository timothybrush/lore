# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging
import os

import pytest

from error_types import RevisionNotFound
from lore import Lore, verify_signatures

logger = logging.getLogger(__name__)


@pytest.mark.smoke
def test_syncfind(new_lore_repo):
    repo: Lore = new_lore_repo()

    # Generate a file
    text_file = "binary-file.txt"

    with repo.open_file(text_file, "w+b") as output_file:
        output_file.write(os.urandom(100))

    repo.stage(scan=True)
    repo.commit("Test commit 1")

    with repo.open_file(text_file, "w+b") as output_file:
        output_file.write(os.urandom(100))

    repo.stage(scan=True)
    repo.commit("Test commit 2")

    with repo.open_file(text_file, "w+b") as output_file:
        output_file.write(os.urandom(100))

    repo.stage(scan=True)
    repo.commit("Test commit 3")
    repo.branch_push()

    # List all revisions
    list_output = repo.revision_history()
    verify_signatures(list_output, 3)

    # Ensure sync to short (7 digit) signature works
    repo.sync(list_output[1].signature[0:7])

    output = repo.status()

    assert list_output[1].signature in output, "Missing revision in status"

    repo.sync()

    # Corrupt final digit of short signature
    if list_output[1].signature[6] == "0":
        digit = "1"
    else:
        digit = "0"

    # Ensure sync to short (7 digit) signature with corrupted final digit fails
    with pytest.raises(RevisionNotFound):
        repo.sync(list_output[1].signature[0:6] + digit)

    repo.sync()
    # Ensure sync to short (7 digit) signature with inadequate search limit fails
    with pytest.raises(RevisionNotFound):
        repo.sync(list_output[1].signature[0:7], search_limit=1)

    output = repo.sync("@LATEST~1")
    assert list_output[1].signature in output, "Failed to sync to LATEST~1"

    output = repo.sync("@LATEST")
    assert list_output[2].signature in output, "Failed to sync to LATEST"

    with pytest.raises(RevisionNotFound):
        repo.sync("@LATEST~3")

    with pytest.raises(RevisionNotFound):
        repo.sync("@TYPO")

    output = repo.sync("@3~1")
    assert list_output[1].signature in output, "Failed to sync to @3~1"

    output = repo.sync("@3")
    assert list_output[2].signature in output, "Failed to sync to @3"

    with pytest.raises(RevisionNotFound):
        repo.sync("@3~3")

    with pytest.raises(RevisionNotFound):
        repo.sync("@3~nonint")

    output = repo.sync(f"{list_output[2].signature}~1")
    assert list_output[1].signature in output, "Failed to sync to <LATEST revision>~1"

    output = repo.sync(f"{list_output[2].signature}")
    assert list_output[2].signature in output, "Failed to sync to <LATEST revision>"

    with pytest.raises(RevisionNotFound):
        repo.sync(f"{list_output[2].signature}~3")
