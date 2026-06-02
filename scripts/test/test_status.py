# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging
import os

import pytest
from test_utils import posix_join

from lore import Lore

logger = logging.getLogger(__name__)


@pytest.mark.smoke
def test_status(new_lore_repo):
    repo: Lore = new_lore_repo()
    for i in range(10):
        subpath = str(i)
        repo.make_dirs(subpath)
        for j in range(10):
            subsubpath = posix_join(subpath, str(j))
            repo.make_dirs(subsubpath)
            with repo.open_file(
                posix_join(subsubpath, "test.uasset"), "w+b"
            ) as output_file:
                output_file.write(os.urandom(i + j + 30))

    repo.stage(scan=True)
    repo.commit()
    repo.repository_verify()

    status_path = "test.txt"
    status_subpath = posix_join("subpath", "another.txt")
    with repo.open_file(status_path, "w+b") as output_file:
        output_file.write(os.urandom(100))
    repo.make_dirs("subpath")
    with repo.open_file(status_subpath, "w+b") as output_file:
        output_file.write(os.urandom(200))

    # Status with path filter to an untracked file
    output = repo.status(status_path, unstaged=True)

    counting = False
    num_unstaged = 0
    for line in output.splitlines():
        if counting:
            num_unstaged += 1
            assert line.startswith("A ") and line.endswith(status_path), (
                f"Filtered status found unexpected modified file path: {line}"
            )
        elif line.rstrip() == "Untracked files:":
            counting = True
        else:
            assert not line.startswith("Changes not staged"), (
                f"Filtered status --unstaged found unrelated changes"
            )
            assert not line.startswith("D ") or line.startswith("M "), (
                f"Filtered status --unstaged found unrelated changes"
            )

    assert num_unstaged == 1, (
        f"Filtered status --unstaged did not return the expected paths, got {num_unstaged} expected 1"
    )

    # Status with path filter to an untracked file in a subdir
    output = repo.status("subpath", unstaged=True)

    counting = False
    num_unstaged = 0
    for line in output.splitlines():
        if counting:
            num_unstaged += 1
            assert line.startswith("A ") and line.endswith(status_subpath), (
                f"Filtered status found unexpected modified file path: {line}"
            )
        elif line.rstrip() == "Untracked files:":
            counting = True
        else:
            assert not line.startswith("Changes not staged"), (
                f"Filtered status --unstaged found unrelated changes"
            )
            assert not line.startswith("D ") or line.startswith("M "), (
                f"Filtered status --unstaged found unrelated changes"
            )

    assert num_unstaged == 1, (
        f"Filtered status --unstaged did not return the expected paths, got {num_unstaged} expected 1"
    )

    # Status with path filter to an untracked file in a subdir
    output = repo.status(status_subpath, unstaged=True)

    counting = False
    num_unstaged = 0
    for line in output.splitlines():
        if counting:
            num_unstaged += 1
            assert line.startswith("A ") and line.endswith(status_subpath), (
                f"Filtered status found unexpected modified file path: {line}"
            )
        elif line.rstrip() == "Untracked files:":
            counting = True
        else:
            assert not line.startswith("Changes not staged"), (
                f"Filtered status --unstaged found unrelated changes"
            )
            assert not line.startswith("D ") or line.startswith("M "), (
                f"Filtered status --unstaged found unrelated changes"
            )

    assert num_unstaged == 1, (
        f"Filtered status --unstaged did not return the expected paths, got {num_unstaged} expected 1"
    )

    repo.stage(scan=True)
    repo.commit()

    with repo.open_file(status_path, "w+b") as output_file:
        output_file.write(os.urandom(1000))
    repo.make_dirs("subpath")
    with repo.open_file(status_subpath, "w+b") as output_file:
        output_file.write(os.urandom(2000))

    # Status with path filter to an untracked file
    output = repo.status(status_path, unstaged=True)

    counting = False
    num_unstaged = 0
    for line in output.splitlines():
        if counting:
            num_unstaged += 1
            assert line.startswith("M ") and line.endswith(status_path), (
                f"Filtered status found unexpected modified file path: {line}"
            )
        elif line.rstrip() == "Changes not staged for commit:":
            counting = True
        else:
            assert not line.startswith("Changes not staged"), (
                f"Filtered status --unstaged found unrelated changes"
            )
            assert not line.startswith("D ") or line.startswith("A "), (
                f"Filtered status --unstaged found unrelated changes"
            )

    assert num_unstaged == 1, (
        f"Filtered status --unstaged did not return the expected paths, got {num_unstaged} expected 1"
    )

    # Status with path filter to an untracked file in a subdir
    output = repo.status("subpath", unstaged=True)

    counting = False
    num_unstaged = 0
    for line in output.splitlines():
        if counting:
            num_unstaged += 1
            assert line.startswith("M ") and line.endswith(status_subpath), (
                f"Filtered status found unexpected modified file path: {line}"
            )
        elif line.rstrip() == "Changes not staged for commit:":
            counting = True
        else:
            assert not line.startswith("Changes not staged"), (
                f"Filtered status --unstaged found unrelated changes"
            )
            assert not line.startswith("D ") or line.startswith("A "), (
                f"Filtered status --unstaged found unrelated changes"
            )

    assert num_unstaged == 1, (
        f"Filtered status --unstaged did not return the expected paths, got {num_unstaged} expected 1"
    )

    # Status with path filter to an untracked file in a subdir
    output = repo.status(status_subpath, unstaged=True)

    counting = False
    num_unstaged = 0
    for line in output.splitlines():
        if counting:
            num_unstaged += 1
            assert line.startswith("M ") and line.endswith(status_subpath), (
                f"Filtered status found unexpected modified file path: {line}"
            )
        elif line.rstrip() == "Changes not staged for commit:":
            counting = True
        else:
            assert not line.startswith("Changes not staged"), (
                f"Filtered status --unstaged found unrelated changes"
            )
            assert not line.startswith("D ") or line.startswith("A "), (
                f"Filtered status --unstaged found unrelated changes"
            )

    assert num_unstaged == 1, (
        f"Filtered status --unstaged did not return the expected paths, got {num_unstaged} expected 1"
    )

    repo.remove_file(status_subpath)

    # Status with path filter to an untracked file in a subdir
    output = repo.status("subpath", unstaged=True)

    counting = False
    num_unstaged = 0
    for line in output.splitlines():
        if counting:
            num_unstaged += 1
            assert line.startswith("D ") and line.endswith(status_subpath), (
                f"Filtered status found unexpected modified file path: {line}"
            )
        elif line.rstrip() == "Changes not staged for commit:":
            counting = True
        else:
            assert not line.startswith("Changes not staged"), (
                f"Filtered status --unstaged found unrelated changes: {line}"
            )
            assert not line.startswith("A ") or line.startswith("M "), (
                f"Filtered status --unstaged found unrelated changes: {line}"
            )

    assert num_unstaged == 1, (
        f"Filtered status --unstaged did not return the expected paths, got {num_unstaged} expected 1"
    )

    # Status with path filter to an untracked file in a subdir
    output = repo.status(status_subpath, unstaged=True)

    counting = False
    num_unstaged = 0
    for line in output.splitlines():
        if counting:
            num_unstaged += 1
            assert line.startswith("D ") and line.endswith(status_subpath), (
                f"Filtered status found unexpected modified file path: {line}"
            )
        elif line.rstrip() == "Changes not staged for commit:":
            counting = True
        else:
            assert not line.startswith("Changes not staged"), (
                f"Filtered status --unstaged found unrelated changes: {line}"
            )
            assert not line.startswith("A ") or line.startswith("M "), (
                f"Filtered status --unstaged found unrelated changes: {line}"
            )

    assert num_unstaged == 1, (
        f"Filtered status --unstaged did not return the expected paths, got {num_unstaged} expected 1"
    )

    # Status to an ignored file path
    with repo.open_file(repo.ignore_file(), "w+") as output_file:
        output_file.writelines(["testpath/"])

    repo.make_dirs("testpath")
    with repo.open_file(posix_join("testpath", "testfile.txt"), "w+") as output_file:
        output_file.writelines(["testing ignore"])

    output = repo.status(posix_join("testpath", "testfile.txt"), unstaged=True)

    assert "Changes not staged for commit:" not in output, (
        "Found unrelated unstaged change when query status for ignored file"
    )
    assert "Untracked files:" not in output, (
        "Found unrelated untracked change when query status for ignored file"
    )


@pytest.mark.smoke
def test_status_revision_only(new_lore_repo):
    repo: Lore = new_lore_repo()

    with repo.open_file("test.txt", "w+b") as f:
        f.write(os.urandom(100))
    repo.stage(scan=True)
    repo.commit()

    # Create staged and unstaged changes
    with repo.open_file("staged.txt", "w+b") as f:
        f.write(os.urandom(100))
    repo.stage(scan=True)
    with repo.open_file("unstaged.txt", "w+b") as f:
        f.write(os.urandom(100))

    # --revision-only should emit revision info but no file changes
    output = repo.status(revision_only=True)

    assert "Repository" in output, "Expected repository header in revision-only output"
    assert "On branch" in output, "Expected branch info in revision-only output"
    assert "Changes staged for commit:" not in output, (
        "revision-only should not show staged changes"
    )
    assert "Changes not staged for commit:" not in output, (
        "revision-only should not show unstaged changes"
    )
    assert "Untracked files:" not in output, (
        "revision-only should not show untracked files"
    )
