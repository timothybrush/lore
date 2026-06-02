# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging
import os

import pytest

from error_types import BranchBehindLatest
from lore import Lore, verify_signatures

logger = logging.getLogger(__name__)


@pytest.mark.smoke
def test_restore(new_lore_repo):
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

    # Commit the files
    repo.commit("Initial snapshot - add text & binary files")

    # Modify a file
    with repo.open_file(binary_file, "w+b") as output_file:
        output_file.write(os.urandom(1000))

    repo.stage(scan=True)
    repo.commit("Snapshot 2 - modify binary file")

    # Modify a file
    with repo.open_file(text_file, "w+") as output_file:
        output_file.writelines(
            ["One line\n", "Another line\n", "Third line\n", "Fourth line\n"]
        )

    repo.stage(scan=True)
    repo.commit("Snapshot 3 - modify text file")
    repo.push()

    repo.remove_file(text_file)

    repo.stage(scan=True)
    repo.commit("Snapshot 4 - delete text file")
    repo.push()

    clone = repo.clone()

    # Modify file
    cloned_binary_file = "binary-File.bin"
    with clone.open_file(cloned_binary_file, "w+b") as output_file:
        output_file.write(os.urandom(1100))

    # Stage the files
    clone.stage(scan=True)
    clone.commit("Snapshot 5 - modify clone binary file")
    clone.push()

    # List all revisions
    output = repo.revision_history(remote=True)

    verify_signatures(output, 5)

    # Sync source repo back to revision 2
    repo.sync(output[1].signature)

    # Try to restore to head in the source repo because not at the latest revision
    with pytest.raises(BranchBehindLatest):
        repo.revision_restore("Restored revision 2 to latest")

    # Sync source repo to local head
    # Because we are behind local head, running 'sync' without any arguments will sync to local head.
    repo.sync()

    # Sync source repo to remote head
    # Because we are on local head, running 'sync' without any arguments will sync to remote head.
    repo.sync()

    # Sync source repo back to revision 2 out of 5
    repo.sync(output[1].signature)

    # Preview list of files affected during restore
    repo.revision_restore(dry_run=True)

    # Restore revision 2
    repo.revision_restore("Restored revision 2 to latest")

    # List all revisions
    output = repo.revision_history()

    verify_signatures(output, 6)

    # Describe current revision with delta's
    repo.revision_info(output[0].signature, delta=True)

    # Show status
    repo.repository_status(unstaged=True)

    # Sync source repo back to revision 2 again
    repo.sync(output[1].signature)

    # Modify a file
    with repo.open_file(binary_file, "w+b") as output_file:
        output_file.write(os.urandom(1200))

    # Restore revision 2 to latest again whilst having pending changes
    repo.revision_restore("Restored revision 2 to latest again")

    # Show status and reset
    repo.repository_status(unstaged=True)

    repo.file_reset()

    # Generate a large number of files and verify restore works
    output = repo.revision_info()
    restore_base_revision = output.signature

    for i in range(10):
        subpath = str(i)
        repo.make_dirs(subpath)
        for j in range(10):
            subsubpath = os.path.join(subpath, str(j))
            repo.make_dirs(subsubpath)
            for k in range(10):
                with repo.open_file(
                    os.path.join(subsubpath, str(k) + ".uasset"), "w+b"
                ) as output_file:
                    if k == 0:
                        output_file.write(os.urandom(500000))
                    else:
                        output_file.write(os.urandom(10))

    repo.stage(scan=True)
    repo.commit("Generated files")
    repo.push()

    output = repo.revision_info()
    restore_back_revision = output.signature

    repo.sync(restore_base_revision)
    repo.revision_restore("Restore base revision without generated files")

    repo.sync(restore_back_revision)
    repo.revision_restore("Restore revision with all generated files")
