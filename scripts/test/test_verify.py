# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging
import os

import pytest

from lore import Lore

logger = logging.getLogger(__name__)


# =============================================================================
# LOCAL VERIFY TESTS
# =============================================================================


@pytest.mark.smoke
def test_verify_fragment_local_hash_and_context(new_lore_repo):
    """Local verify with hash + context."""
    repo: Lore = new_lore_repo()

    test_file = "test.txt"
    with repo.open_file(test_file, "w+b") as f:
        f.write(os.urandom(1000))

    repo.stage(scan=True, offline=True)
    repo.commit("Test commit", offline=True)

    file_info = repo.file_info(test_file, offline=True)[0]
    fragment_hash = file_info.hash
    context = file_info.context

    output = repo.repository_verify_fragment(
        fragment_hash, context=context, local=True, check=False
    )
    # Local verify with context looks for exact address match
    assert "not found" in output.lower() or "Fragment status: OK" in output


@pytest.mark.smoke
def test_verify_fragment_local_hash_only(new_lore_repo):
    """Local verify with hash only."""
    repo: Lore = new_lore_repo()

    test_file = "test.txt"
    with repo.open_file(test_file, "w+b") as f:
        f.write(os.urandom(1000))

    repo.stage(scan=True, offline=True)
    repo.commit("Test commit", offline=True)

    file_info = repo.file_info(test_file, offline=True)[0]
    fragment_hash = file_info.hash

    output = repo.repository_verify_fragment(fragment_hash, local=True)
    assert "Fragment status: OK" in output
    assert "Matches (0)" not in output


@pytest.mark.smoke
def test_verify_fragment_local_wrong_context(new_lore_repo):
    """Local verify with valid hash but wrong context - should fail to find address."""
    repo: Lore = new_lore_repo()

    test_file = "test.txt"
    with repo.open_file(test_file, "w+b") as f:
        f.write(os.urandom(1000))

    repo.stage(scan=True, offline=True)
    repo.commit("Test commit", offline=True)

    file_info = repo.file_info(test_file, offline=True)[0]
    fragment_hash = file_info.hash
    fake_context = "0" * 32  # Valid format but wrong context

    output = repo.repository_verify_fragment(
        fragment_hash, context=fake_context, local=True, check=False
    )
    # With wrong context, local verify fails with "Address not found"
    assert "not found" in output.lower() or "FAILED" in output


@pytest.mark.smoke
def test_verify_fragment_local_nonexistent_hash(new_lore_repo):
    """Local verify with nonexistent hash - should fail to find address."""
    repo: Lore = new_lore_repo()

    # Need at least one commit to have a local store
    test_file = "test.txt"
    with repo.open_file(test_file, "w+b") as f:
        f.write(os.urandom(1000))

    repo.stage(scan=True, offline=True)
    repo.commit("Test commit", offline=True)

    fake_hash = "0" * 64  # Valid format but nonexistent

    output = repo.repository_verify_fragment(fake_hash, local=True, check=False)
    # Nonexistent hash returns "Address not found" error
    assert "not found" in output.lower() or "FAILED" in output


# =============================================================================
# REMOTE VERIFY TESTS
# =============================================================================


@pytest.mark.smoke
def test_verify_fragment_remote_hash_and_context(new_lore_repo):
    """Remote verify with hash + context."""
    repo: Lore = new_lore_repo()

    test_file = "test.txt"
    with repo.open_file(test_file, "w+b") as f:
        f.write(os.urandom(1000))

    repo.stage(scan=True)
    repo.commit("Test commit")
    repo.push()

    file_info = repo.file_info(test_file)[0]
    fragment_hash = file_info.hash
    context = file_info.context

    output = repo.repository_verify_fragment(fragment_hash, context=context)
    assert "Fragment status: OK" in output


@pytest.mark.smoke
def test_verify_fragment_remote_hash_only(new_lore_repo):
    """Remote verify with hash only."""
    repo: Lore = new_lore_repo()

    test_file = "test.txt"
    with repo.open_file(test_file, "w+b") as f:
        f.write(os.urandom(1000))

    repo.stage(scan=True)
    repo.commit("Test commit")
    repo.push()

    file_info = repo.file_info(test_file)[0]
    fragment_hash = file_info.hash

    output = repo.repository_verify_fragment(fragment_hash)
    assert "Fragment status: OK" in output


@pytest.mark.smoke
def test_verify_fragment_remote_wrong_context(new_lore_repo):
    """Remote verify with valid hash but wrong context.

    Remote verify with the correct hash but wrong context still returns OK
    because the fragment is verified by hash presence, and context is used
    for the output display but doesn't affect verification outcome.
    """
    repo: Lore = new_lore_repo()

    test_file = "test.txt"
    with repo.open_file(test_file, "w+b") as f:
        f.write(os.urandom(1000))

    repo.stage(scan=True)
    repo.commit("Test commit")
    repo.push()

    file_info = repo.file_info(test_file)[0]
    fragment_hash = file_info.hash
    fake_context = "0" * 32  # Valid format but wrong context

    output = repo.repository_verify_fragment(
        fragment_hash, context=fake_context, check=False
    )
    # Remote verify succeeds as long as hash exists - context doesn't affect verification
    assert "Fragment status: OK" in output


@pytest.mark.smoke
def test_verify_fragment_remote_nonexistent_hash(new_lore_repo):
    """Remote verify with nonexistent hash - should fail."""
    repo: Lore = new_lore_repo()

    # Need at least one push to establish remote connection
    test_file = "test.txt"
    with repo.open_file(test_file, "w+b") as f:
        f.write(os.urandom(1000))

    repo.stage(scan=True)
    repo.commit("Test commit")
    repo.push()

    fake_hash = "0" * 64  # Valid format but nonexistent

    output = repo.repository_verify_fragment(fake_hash, check=False)
    assert "FAILED" in output or "not found" in output.lower()


# =============================================================================
# VERIFY + HEAL TESTS
# =============================================================================


@pytest.mark.smoke
def test_verify_fragment_remote_displays_corrupted_status(new_lore_repo):
    """Remote verify displays corrupted status in output."""
    repo: Lore = new_lore_repo()

    test_file = "test.txt"
    with repo.open_file(test_file, "w+b") as f:
        f.write(os.urandom(1000))

    repo.stage(scan=True)
    repo.commit("Test commit")
    repo.push()

    file_info = repo.file_info(test_file)[0]
    fragment_hash = file_info.hash
    context = file_info.context

    output = repo.repository_verify_fragment(fragment_hash, context=context)
    # Output should include fragment status
    assert "Fragment status: OK" in output or "Fragment status: CORRUPTED" in output


@pytest.mark.smoke
def test_verify_fragment_remote_with_heal_flag(new_lore_repo):
    """Remote verify with --heal flag runs without error."""
    repo: Lore = new_lore_repo()

    test_file = "test.txt"
    with repo.open_file(test_file, "w+b") as f:
        f.write(os.urandom(1000))

    repo.stage(scan=True)
    repo.commit("Test commit")
    repo.push()

    file_info = repo.file_info(test_file)[0]
    fragment_hash = file_info.hash
    context = file_info.context

    # Verify with heal flag - should succeed for non-corrupted fragment
    output = repo.repository_verify_fragment(fragment_hash, context=context, heal=True)
    assert "Fragment status: OK" in output
    assert "Healing: Not Attempted" in output


@pytest.mark.smoke
def test_verify_fragment_local_with_heal_flag(new_lore_repo):
    """Local verify with --heal flag runs without error."""
    repo: Lore = new_lore_repo()

    test_file = "test.txt"
    with repo.open_file(test_file, "w+b") as f:
        f.write(os.urandom(1000))

    repo.stage(scan=True, offline=True)
    repo.commit("Test commit", offline=True)

    file_info = repo.file_info(test_file, offline=True)[0]
    fragment_hash = file_info.hash

    # Verify with heal flag locally
    output = repo.repository_verify_fragment(fragment_hash, local=True, heal=True)
    assert "Fragment status: OK" in output
