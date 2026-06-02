# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import json
import logging
import os

import pytest
from test_utils import to_posix
from lore_parsers import parse_jsonl, parse_status_json

from lore import Lore

logger = logging.getLogger(__name__)

def has_staged_anchor(repo: Lore) -> bool:
    """Check whether the repository has a staged revision by querying status."""
    output = repo.status(json=True, offline=True)
    zero_hash = "0" * 64
    for line in output.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        data = event.get("data", {})
        revision_staged = data.get("revisionStaged", "")
        if revision_staged and revision_staged != zero_hash:
            return True
    return False


@pytest.mark.smoke
def test_unstage_clears_staged_state(new_lore_repo):
    """Verify that unstaging the last staged item fully clears staged state,
    regardless of whether the unstage target is a directory or a file.
    The staged state must be empty when nothing remains staged."""

    repo: Lore = new_lore_repo()

    # Setup: initial commit so main has content
    with repo.open_file("initial.txt", "w+") as f:
        f.write("initial content\n")
    repo.stage(scan=True)
    repo.commit()
    repo.push()

    assert not has_staged_anchor(repo), "No staged anchor should exist after commit"

    repo.branch_create("test-unstage")

    # Scenario 1: Stage file in new directory, unstage the directory
    new_dir = "newDir"
    new_dir_file = os.path.join(new_dir, "file.txt")
    repo.make_dirs(new_dir)
    with repo.open_file(new_dir_file, "w+") as f:
        f.write("content in new directory\n")

    repo.stage(new_dir_file)
    assert has_staged_anchor(repo), "Staged anchor should exist after staging"

    repo.unstage(new_dir)

    assert not has_staged_anchor(repo), (
        "Staged anchor should be removed after unstaging the only staged directory"
    )

    repo.branch_switch("main")
    repo.branch_switch("test-unstage")

    # Scenario 2: Stage file in new directory, unstage the file directly —
    # the parent directory remains staged (it's also a StagedAdd), then unstage it too
    another_dir = "anotherDir"
    another_file = os.path.join(another_dir, "file.txt")
    repo.make_dirs(another_dir)
    with repo.open_file(another_file, "w+") as f:
        f.write("content in another directory\n")

    repo.stage(another_file)
    assert has_staged_anchor(repo), "Staged anchor should exist after staging"

    repo.unstage(another_file)

    assert has_staged_anchor(repo), (
        "Staged anchor should still exist — parent directory is still staged"
    )

    staged_after = parse_status_json(repo.status(json=True))
    staged_paths_after = [e["path"] for e in staged_after]
    assert to_posix(another_file) not in staged_paths_after, "File should no longer be staged"
    assert another_dir in staged_paths_after, (
        "Parent directory should still be staged (it is also a new add)"
    )

    repo.unstage(another_dir)

    assert not has_staged_anchor(repo), (
        "Staged anchor should be removed after unstaging the parent directory"
    )

    repo.branch_switch("main")
    repo.branch_switch("test-unstage")

    # Scenario 3: Stage two directories, unstage one at a time by directory path
    dir_a = "dirA"
    dir_b = "dirB"
    file_a = os.path.join(dir_a, "a.txt")
    file_b = os.path.join(dir_b, "b.txt")

    repo.make_dirs(dir_a)
    repo.make_dirs(dir_b)
    with repo.open_file(file_a, "w+") as f:
        f.write("file A\n")
    with repo.open_file(file_b, "w+") as f:
        f.write("file B\n")

    repo.stage(file_a)
    repo.stage(file_b)
    assert has_staged_anchor(repo), "Staged anchor should exist after staging"

    # Unstage first directory — second should remain staged
    repo.unstage(dir_a)

    assert has_staged_anchor(repo), (
        "Staged anchor should still exist — dirB is still staged"
    )

    staged_partial = parse_status_json(repo.status(json=True))
    staged_paths_partial = [e["path"] for e in staged_partial]
    assert to_posix(file_a) not in staged_paths_partial, "File A should no longer be staged"
    assert dir_a not in staged_paths_partial, "Dir A should no longer be staged"
    assert to_posix(file_b) in staged_paths_partial, "File B should remain staged"

    # Unstage second directory — nothing should remain staged
    repo.unstage(dir_b)

    assert not has_staged_anchor(repo), (
        "Staged anchor should be removed after unstaging the last staged directory"
    )

    repo.branch_switch("main")


def get_unstage_counts(output: str) -> dict:
    """Extract the count object from the fileUnstageEnd event in JSON output."""
    events = parse_jsonl(output, "fileUnstageEnd")
    assert len(events) == 1, f"Expected 1 fileUnstageEnd event, got {len(events)}"
    return events[0]["count"]


def get_unstage_file_events(output: str) -> list[dict]:
    """Extract all fileUnstageFile events from JSON output."""
    return parse_jsonl(output, "fileUnstageFile")


@pytest.mark.smoke
def test_unstage_discard_counts(new_lore_repo):
    """Verify that unstage reports correct discard and unstage counts in the
    fileUnstageEnd event for various scenarios: new files, committed files,
    directories with files, and nested directories."""

    repo: Lore = new_lore_repo()

    # Setup: initial commit with a file so we can test unstage of committed files
    with repo.open_file("committed.txt", "w+") as f:
        f.write("committed content\n")
    repo.stage(scan=True)
    repo.commit()
    repo.push()

    repo.branch_create("test-discard-counts")

    # Scenario 1: Unstage a new file (clear path — last staged item)
    with repo.open_file("new_file.txt", "w+") as f:
        f.write("new content\n")

    repo.stage("new_file.txt")
    output = repo.unstage("new_file.txt", json=True)
    counts = get_unstage_counts(output)

    assert counts["fileDiscardedCount"] == 1, (
        f"Scenario 1: expected fileDiscardedCount=1, got {counts['fileDiscardedCount']}"
    )
    assert counts["fileUnstagedCount"] == 0, (
        f"Scenario 1: expected fileUnstagedCount=0, got {counts['fileUnstagedCount']}"
    )
    assert counts["directoryDiscardedCount"] == 0
    assert counts["directoryUnstagedCount"] == 0

    repo.branch_switch("main")
    repo.branch_switch("test-discard-counts")

    # Scenario 2: Unstage a new file while another committed file is also staged (non-clear path)
    with repo.open_file("new_file2.txt", "w+") as f:
        f.write("another new file\n")

    # Modify the committed file so it can be staged
    with repo.open_file("committed.txt", "w+") as f:
        f.write("modified committed content\n")

    repo.stage("new_file2.txt")
    repo.stage("committed.txt")

    # Unstage only the new file — committed.txt remains staged, so clear=false
    output = repo.unstage("new_file2.txt", json=True)
    counts = get_unstage_counts(output)

    assert counts["fileDiscardedCount"] == 1, (
        f"Scenario 2: expected fileDiscardedCount=1, got {counts['fileDiscardedCount']}"
    )
    assert counts["fileUnstagedCount"] == 0, (
        f"Scenario 2: expected fileUnstagedCount=0, got {counts['fileUnstagedCount']}"
    )

    # Clean up: unstage committed.txt too
    repo.unstage("committed.txt")

    repo.branch_switch("main")
    repo.branch_switch("test-discard-counts")

    # Scenario 3: Unstage a modified committed file (unstage, not discard)
    with repo.open_file("committed.txt", "w+") as f:
        f.write("modified again\n")

    repo.stage("committed.txt")
    output = repo.unstage("committed.txt", json=True)
    counts = get_unstage_counts(output)

    assert counts["fileUnstagedCount"] == 1, (
        f"Scenario 3: expected fileUnstagedCount=1, got {counts['fileUnstagedCount']}"
    )
    assert counts["fileDiscardedCount"] == 0, (
        f"Scenario 3: expected fileDiscardedCount=0, got {counts['fileDiscardedCount']}"
    )

    repo.branch_switch("main")
    repo.branch_switch("test-discard-counts")

    # Scenario 4: Unstage a new directory with 1 file
    dir1 = "newdir1"
    dir1_file = os.path.join(dir1, "file.txt")
    repo.make_dirs(dir1)
    with repo.open_file(dir1_file, "w+") as f:
        f.write("file in new dir\n")

    repo.stage(dir1_file)
    output = repo.unstage(dir1, json=True)
    counts = get_unstage_counts(output)

    assert counts["directoryDiscardedCount"] == 1, (
        f"Scenario 4: expected directoryDiscardedCount=1, got {counts['directoryDiscardedCount']}"
    )
    assert counts["fileDiscardedCount"] == 1, (
        f"Scenario 4: expected fileDiscardedCount=1, got {counts['fileDiscardedCount']}"
    )

    file_events = get_unstage_file_events(output)
    event_paths = [e["path"] for e in file_events]
    assert event_paths == [to_posix(dir1_file)], (
        f"Scenario 4: expected event for {dir1_file}, got {event_paths}"
    )

    repo.branch_switch("main")
    repo.branch_switch("test-discard-counts")

    # Scenario 5: Unstage a new directory with 3 files
    dir2 = "newdir2"
    repo.make_dirs(dir2)
    for i in range(3):
        with repo.open_file(os.path.join(dir2, f"file{i}.txt"), "w+") as f:
            f.write(f"content {i}\n")

    repo.stage(os.path.join(dir2, "file0.txt"))
    repo.stage(os.path.join(dir2, "file1.txt"))
    repo.stage(os.path.join(dir2, "file2.txt"))
    output = repo.unstage(dir2, json=True)
    counts = get_unstage_counts(output)

    assert counts["directoryDiscardedCount"] == 1, (
        f"Scenario 5: expected directoryDiscardedCount=1, got {counts['directoryDiscardedCount']}"
    )
    assert counts["fileDiscardedCount"] == 3, (
        f"Scenario 5: expected fileDiscardedCount=3, got {counts['fileDiscardedCount']}"
    )

    file_events = get_unstage_file_events(output)
    event_paths = sorted([e["path"] for e in file_events])
    expected_paths = sorted([to_posix(os.path.join(dir2, f"file{i}.txt")) for i in range(3)])
    assert event_paths == expected_paths, (
        f"Scenario 5: expected events for {expected_paths}, got {event_paths}"
    )
    assert all(e["action"] == "delete" for e in file_events), (
        "Scenario 5: all events should have action=delete"
    )

    repo.branch_switch("main")
    repo.branch_switch("test-discard-counts")

    # Scenario 6: Unstage a nested dir/subdir/file.txt
    nested_dir = "nested"
    nested_subdir = os.path.join(nested_dir, "subdir")
    nested_file = os.path.join(nested_subdir, "deep.txt")
    repo.make_dirs(nested_subdir)
    with repo.open_file(nested_file, "w+") as f:
        f.write("deeply nested\n")

    repo.stage(nested_file)
    output = repo.unstage(nested_dir, json=True)
    counts = get_unstage_counts(output)

    assert counts["directoryDiscardedCount"] == 2, (
        f"Scenario 6: expected directoryDiscardedCount=2, got {counts['directoryDiscardedCount']}"
    )
    assert counts["fileDiscardedCount"] == 1, (
        f"Scenario 6: expected fileDiscardedCount=1, got {counts['fileDiscardedCount']}"
    )

    # Events should include both the discarded subdirectory and the file
    file_events = get_unstage_file_events(output)
    event_paths = sorted([e["path"] for e in file_events])
    expected_paths = sorted([to_posix(nested_subdir), to_posix(nested_file)])
    assert event_paths == expected_paths, (
        f"Scenario 6: expected events for {expected_paths}, got {event_paths}"
    )
    assert to_posix(nested_subdir) in event_paths, (
        f"Scenario 6: discarded directory {nested_subdir} should have an event"
    )

    repo.branch_switch("main")
    repo.branch_switch("test-discard-counts")

    # Scenario 7: Unstage multiple new files at once
    for i in range(4):
        with repo.open_file(f"multi_{i}.txt", "w+") as f:
            f.write(f"multi content {i}\n")
        repo.stage(f"multi_{i}.txt")

    output = repo.unstage(json=True)
    counts = get_unstage_counts(output)

    assert counts["fileDiscardedCount"] == 4, (
        f"Scenario 7: expected fileDiscardedCount=4, got {counts['fileDiscardedCount']}"
    )
    assert counts["fileUnstagedCount"] == 0
    assert counts["directoryDiscardedCount"] == 0

    repo.branch_switch("main")
    repo.branch_switch("test-discard-counts")

    # Scenario 8: Deep nested structure — files at multiple levels get individual events
    # Structure: deep/a.txt, deep/mid/b.txt, deep/mid/bottom/c.txt
    deep = "deep"
    mid = os.path.join(deep, "mid")
    bottom = os.path.join(mid, "bottom")
    repo.make_dirs(bottom)

    deep_files = {
        os.path.join(deep, "a.txt"): "file at top",
        os.path.join(mid, "b.txt"): "file at mid",
        os.path.join(bottom, "c.txt"): "file at bottom",
    }
    for path, content in deep_files.items():
        with repo.open_file(path, "w+") as f:
            f.write(content + "\n")

    for path in deep_files:
        repo.stage(path)

    output = repo.unstage(deep, json=True)
    counts = get_unstage_counts(output)

    # 3 directories: deep, mid, bottom (deep itself is counted in unstage_node,
    # mid and bottom are counted via discard_subnodes)
    assert counts["directoryDiscardedCount"] == 3, (
        f"Scenario 8: expected directoryDiscardedCount=3, got {counts['directoryDiscardedCount']}"
    )
    assert counts["fileDiscardedCount"] == 3, (
        f"Scenario 8: expected fileDiscardedCount=3, got {counts['fileDiscardedCount']}"
    )

    # Every node must get its own event, including nested directories
    file_events = get_unstage_file_events(output)
    event_paths = sorted([e["path"] for e in file_events])
    expected_paths = sorted([to_posix(p) for p in deep_files] + [to_posix(mid), to_posix(bottom)])
    assert event_paths == expected_paths, (
        f"Scenario 8: expected events for {expected_paths}, got {event_paths}"
    )

    repo.branch_switch("main")
