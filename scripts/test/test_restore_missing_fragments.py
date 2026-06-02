# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging

import pytest

from lore import Lore

logger = logging.getLogger(__name__)


@pytest.mark.smoke
def test_restore_missing_fragments(new_lore_repo):
    repo: Lore = new_lore_repo()
    test_file = "test.txt"
    with repo.open_file(test_file, "w+") as output_file:
        output_file.writelines(["One line\n", "Another line\n"])

    repo.stage(scan=True)
    repo.commit()
    repo.push()

    repo.remove_file(test_file)

    repo.stage(scan=True)
    repo.commit()
    repo.push()

    # Clone repository 1
    clone1 = repo.clone()

    with clone1.open_file(test_file, "w+") as output_file:
        output_file.writelines(["One line\n", "Another line\n"])

    clone1.stage(scan=True)
    clone1.commit()
    clone1.push()

    clone1.sync("@2")
    clone1.revision_restore("Restoring 2, removing file")

    clone1.revision_sync("@1")
    clone1.revision_restore("Restoring 1, adding file")

    # Clone repository 2
    _clone2 = repo.clone()
