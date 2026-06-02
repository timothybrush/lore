# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import http.client
import logging
import os
import re

import pytest

from lore import Lore

logger = logging.getLogger(__name__)


@pytest.mark.smoke
def test_rest(new_lore_repo, request, lore_main_server_ports):
    repo: Lore = new_lore_repo()
    # Generate a file
    file = "some-file.bin"

    contents = os.urandom(128 * 1024 * 1024)
    with repo.open_file(file, "wb+") as output_file:
        output_file.write(contents)

    repo.stage(scan=True)
    repo.commit()
    repo.push(level="debug")

    describe_output = repo.file_info(file)[0]

    assert describe_output.context != "", "Context not found in show output"
    assert describe_output.hash != "", "Hash not found in show output"

    status_output = repo.status()

    match = re.search(r"Repository ([a-f0-9]+)", status_output)
    repository_id = match.group(1) if match else None
    assert repository_id is not None, "Repository Id not found in status output"

    hostname = request.config.getoption("--lore-server-hostname")
    port = lore_main_server_ports["http"]

    logger.info(f"Hostname: {hostname}")
    logger.info(f"Port: {port}")

    conn = http.client.HTTPConnection(hostname, port)

    url = f"/v1/repository/{repository_id}/content/{describe_output.hash}-{describe_output.context}"
    logger.info(f"Repository ID: {repository_id}")
    logger.info(f"Content hash: {describe_output.hash}")
    logger.info(f"Content context: {describe_output.context}")
    logger.info(f"Request URL: http://{hostname}:{port}{url}")

    conn.request("GET", url)
    response = conn.getresponse()

    logger.info(f"Status: {response.status}")

    data = response.read()

    conn.close()

    if data != contents:
        pytest.fail(
            f"Returned data did not match local file contents. "
            f"Expected {len(contents)} bytes, got {len(data)} bytes. "
            f"Response preview: {data[:200]!r}"
        )
