#!/usr/bin/python3
# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging
import subprocess
import concurrent.futures

import pytest

from lore import Lore

logger = logging.getLogger(__name__)


def notification_subscribe(
    repo: Lore, timeout: int, expected_messages: list[str] | None = None
):
    """Subscribe to notifications, exiting early when all expected_messages are found."""
    command_args = [
        repo.lore_executable_path,
        "--repository",
        repo.path,
        "--debug",
        "notification",
        "subscribe",
        str(timeout),
    ]

    logger.info(f"Starting notification subscribe: {' '.join(command_args)}")

    process = subprocess.Popen(
        command_args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True
    )

    collected_output = ""
    try:
        if expected_messages is None:
            stdout, _ = process.communicate()
            return stdout

        remaining = set(expected_messages)
        for line in process.stdout:
            collected_output += line
            logger.info(f"notification output: {line.rstrip()}")
            remaining = {msg for msg in remaining if msg not in collected_output}
            if not remaining:
                break
    finally:
        logger.info(f"notification final output: {collected_output!r}")
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()

    return collected_output


@pytest.mark.smoke
class TestNotifications:
    def test_branch_delete_event(
        self,
        new_lore_repo,
    ):
        repo: Lore = new_lore_repo("branch_delete_test")
        child_branch_name = "test-branch"

        with concurrent.futures.ThreadPoolExecutor() as executor:
            notification_future = executor.submit(
                notification_subscribe,
                repo,
                30,
                ["Branch pushed", "Branch deleted"],
            )

            # set up some stub data on main
            text_file = "text-File.txt"
            with repo.open_file(text_file, "w+") as file:
                file.writelines(["One line"])
            repo.stage(scan=True)
            repo.commit()
            repo.push()

            # create a child branch with some extra data
            repo.branch_create(child_branch_name)
            with repo.open_file(text_file, "w+") as file:
                file.writelines(["Two line"])
            repo.stage(scan=True)
            repo.commit()
            # push to raise the 'pushed' notification
            repo.push()

            # go back to main and delete the branch to raise the 'deleted' notification
            repo.branch_switch("main")
            repo.branch_delete(child_branch_name)

            # get the notification output
            notification_output = notification_future.result()

        assert "Branch pushed" in notification_output
        assert "Branch deleted" in notification_output

    def test_branch_created_event(
        self,
        new_lore_repo,
    ):
        repo: Lore = new_lore_repo("branch_CREATED_test")
        child_branch_name = "test-branch"

        with concurrent.futures.ThreadPoolExecutor() as executor:
            notification_future = executor.submit(
                notification_subscribe, repo, 30, ["Branch created"]
            )

            # set up some stub data on main
            text_file = "text-File.txt"
            with repo.open_file(text_file, "w+") as file:
                file.writelines(["One line"])
            repo.stage(scan=True)
            repo.commit()
            repo.push()

            # create a child branch
            repo.branch_create(child_branch_name)

            repo.push()

            # get the notification output
            notification_output = notification_future.result()

        assert "Branch created" in notification_output
