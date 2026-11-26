# verifies(L1): gcp/secretmanager: projects.secrets.create, projects.secrets.get, projects.secrets.list, projects.secrets.patch, projects.secrets.delete
# verifies(L1): gcp/secretmanager: projects.locations.secrets.create, projects.locations.secrets.get, projects.locations.secrets.list, projects.locations.secrets.patch, projects.locations.secrets.delete
import pytest
from google.api_core import exceptions as ex
from google.api_core.client_options import ClientOptions
from google.auth.credentials import AnonymousCredentials
from google.cloud import secretmanager
from google.protobuf import field_mask_pb2


def _client(mcle):
    return secretmanager.SecretManagerServiceClient(
        transport="rest", credentials=AnonymousCredentials(), client_options=ClientOptions(api_endpoint=mcle)
    )


def test_secret_lifecycle(mcle):
    c = _client(mcle)
    s = c.create_secret(request={"parent": "projects/p1", "secret_id": "s1",
                                 "secret": {"replication": {"automatic": {}}, "labels": {"a": "b"}}})
    assert s.name == "projects/p1/secrets/s1"
    assert dict(s.labels) == {"a": "b"}
    assert s.create_time is not None

    assert c.get_secret(name="projects/p1/secrets/s1").name == s.name
    updated = c.update_secret(request={"secret": {"name": s.name, "labels": {"x": "y"}},
                                       "update_mask": field_mask_pb2.FieldMask(paths=["labels"])})
    assert dict(updated.labels) == {"x": "y"}
    assert [x.name for x in c.list_secrets(parent="projects/p1")] == [s.name]

    with pytest.raises(ex.Conflict):
        c.create_secret(request={"parent": "projects/p1", "secret_id": "s1", "secret": {}})
    c.delete_secret(name=s.name)
    with pytest.raises(ex.NotFound):
        c.get_secret(name=s.name)


def test_regional_secret_lifecycle(mcle):
    c = _client(mcle)
    parent = "projects/p2/locations/us-east1"
    s = c.create_secret(request={"parent": parent, "secret_id": "r1", "secret": {"labels": {"a": "b"}}})
    assert s.name == f"{parent}/secrets/r1"
    assert c.get_secret(name=s.name).labels["a"] == "b"
    assert [x.name for x in c.list_secrets(parent=parent)] == [s.name]
    assert list(c.list_secrets(parent="projects/p2")) == []
    c.delete_secret(name=s.name)
    with pytest.raises(ex.NotFound):
        c.get_secret(name=s.name)
