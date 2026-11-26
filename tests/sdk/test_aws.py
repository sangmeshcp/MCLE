# verifies(L1): aws/secretsmanager: CreateSecret, DescribeSecret, ListSecrets, UpdateSecret, DeleteSecret
import boto3
import pytest


@pytest.fixture
def sm(sandbox):
    url, sb = sandbox
    return boto3.client(
        "secretsmanager",
        endpoint_url=url,
        region_name="us-east-1",
        aws_access_key_id=sb["access_key"],
        aws_secret_access_key="test",
    )


def test_secret_lifecycle(sm):
    created = sm.create_secret(Name="db/pass", SecretString="x", Description="d", Tags=[{"Key": "a", "Value": "b"}])
    assert created["ARN"].startswith("arn:aws:secretsmanager:us-east-1:000000000000:secret:db/pass-")
    assert created["Name"] == "db/pass"

    desc = sm.describe_secret(SecretId="db/pass")
    assert desc["Description"] == "d"
    assert desc["Tags"] == [{"Key": "a", "Value": "b"}]
    assert "SecretString" not in desc
    assert sm.describe_secret(SecretId=created["ARN"])["Name"] == "db/pass"

    sm.update_secret(SecretId="db/pass", Description="d2")
    assert sm.describe_secret(SecretId="db/pass")["Description"] == "d2"
    assert [s["Name"] for s in sm.list_secrets()["SecretList"]] == ["db/pass"]

    with pytest.raises(sm.exceptions.ResourceExistsException):
        sm.create_secret(Name="db/pass")

    deleted = sm.delete_secret(SecretId="db/pass", ForceDeleteWithoutRecovery=True)
    assert "DeletionDate" in deleted
    with pytest.raises(sm.exceptions.ResourceNotFoundException):
        sm.describe_secret(SecretId="db/pass")


def test_sandboxes_are_isolated(sm, mcle):
    sm.create_secret(Name="only-here")
    other = boto3.client("secretsmanager", endpoint_url=mcle, region_name="us-east-1",
                         aws_access_key_id="test", aws_secret_access_key="test")
    assert all(s["Name"] != "only-here" for s in other.list_secrets()["SecretList"])


def test_unimplemented_op_is_loud(sm):
    sm.create_secret(Name="s")
    with pytest.raises(Exception) as e:
        sm.get_secret_value(SecretId="s")
    assert "NotImplemented" in str(e.value)
