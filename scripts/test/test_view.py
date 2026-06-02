# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging
import os

import pytest

from lore import Lore

logger = logging.getLogger(__name__)


@pytest.mark.smoke
def test_view(new_lore_repo, tmp_path_factory):
    view_dir = tmp_path_factory.mktemp("view")
    repo: Lore = new_lore_repo()
    # Generate some files
    text_file = "text-File.txt"
    unicode_dir = "奇怪的路徑"
    unicode_file = os.path.join(unicode_dir, "کاراکترهای یونیکد")
    first_dir = "aaaa"
    second_dir = "bbbb"

    with repo.open_file(text_file, "w+") as output_file:
        output_file.writelines(["One line\n", "Another line\n", "Third line\n"])

    repo.make_dirs(os.path.dirname(unicode_file))
    with repo.open_file(unicode_file, "w+", encoding="utf-8") as output_file:
        output_file.writelines(["只需將一些文本寫入文件即可\n"])

    for i in range(4):
        subpath = os.path.join(first_dir, str(i))
        repo.make_dirs(subpath)
        for j in range(5):
            with repo.open_file(
                os.path.join(subpath, str(j) + ".uasset"), "w+b"
            ) as output_file:
                output_file.write(os.urandom(1024))

        subpath = os.path.join(second_dir, str(i))
        repo.make_dirs(subpath)
        for j in range(5):
            with repo.open_file(
                os.path.join(subpath, str(j) + ".uasset"), "w+b"
            ) as output_file:
                output_file.write(os.urandom(1024))

    repo.stage(scan=True)
    repo.commit()
    repo.push()

    # Create a view filter
    view_path = os.path.join(view_dir, "view.txt")
    with open(view_path, "w+") as view_file:
        view_file.write("**\n")
        view_file.write("!" + second_dir + "/1/**\n")

    # Clone the repository with a view filter
    clone = repo.clone(view=view_path)

    os.unlink(view_path)

    # Verify files contents, mode and last modified timestamp
    for index in range(5):
        assert repo.compare_file(
            clone, os.path.join(second_dir, "1", str(index) + ".uasset")
        )

    assert not os.path.exists(os.path.join(clone.path, first_dir)), (
        "Directory not filtered out as expected: " + os.path.join(clone.path, first_dir)
    )
    assert not os.path.exists(os.path.join(clone.path, text_file)), (
        "Top level file not filtered out as expected: "
        + os.path.join(clone.path, text_file)
    )
    assert not os.path.exists(os.path.join(clone.path, unicode_dir)), (
        "Directory not filtered out as expected: "
        + os.path.join(clone.path, unicode_dir)
    )

    for index in range(4):
        if index == 1:
            continue
        test_path2 = os.path.join(clone.path, second_dir, str(index))
        assert not os.path.exists(test_path2), (
            "Directory not filtered out as expected: " + test_path2
        )

    # Modify and stage some files in a branch in the filtered repository clone
    clone.branch_create("test-filter")

    with clone.open_file(
        os.path.join(second_dir, "1", "1.uasset"), "w+b"
    ) as output_file:
        output_file.write(os.urandom(1024))

    os.unlink(os.path.join(clone.path, second_dir, "1", "2.uasset"))

    with clone.open_file(
        os.path.join(second_dir, "1", "3.uasset"), "w+b"
    ) as output_file:
        output_file.write(os.urandom(1024))

    clone.stage(os.path.join(second_dir, "1", "1.uasset"))
    clone.commit("Modification commit")

    clone.stage(scan=True)
    clone.commit("Second modification commit")
    clone.push()

    repo.branch_switch("test-filter")
    repo.sync()

    # Verify files contents, mode and last modified timestamp

    for i in range(5):
        if i == 2:
            test_path = os.path.join(repo.path, second_dir, "1", str(i) + ".uasset")
            assert not os.path.exists(test_path), (
                "File not deleted as expected: " + test_path
            )
        else:
            assert repo.compare_file(
                clone, os.path.join(second_dir, "1", str(i) + ".uasset")
            )

    assert os.path.exists(os.path.join(repo.path, first_dir)), (
        "Directory not retained as expected: " + os.path.join(repo.path, first_dir)
    )

    assert os.path.exists(os.path.join(repo.path, text_file)), (
        "Top level file not retained as expected: " + os.path.join(repo.path, text_file)
    )

    assert os.path.exists(os.path.join(repo.path, unicode_dir)), (
        "Directory not retained as expected: " + os.path.join(repo.path, unicode_dir)
    )

    for i in range(4):
        if i == 1:
            continue
        test_path2 = os.path.join(repo.path, second_dir, str(i))
        assert os.path.exists(test_path2), (
            "Directory not retained as expected: " + test_path2
        )
