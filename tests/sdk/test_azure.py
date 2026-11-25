# verifies(L1): azure/arm: ResourceGroups_CreateOrUpdate, ResourceGroups_Get, ResourceGroups_CheckExistence, ResourceGroups_List, ResourceGroups_Delete, Resources_CreateOrUpdate, Resources_Get, Resources_ListByResourceGroup
import pytest
from azure.core.exceptions import ResourceNotFoundError
from azure.core.pipeline.policies import SansIOHTTPPolicy
from azure.mgmt.resource.resources import ResourceManagementClient

SUB = "00000000-0000-0000-0000-000000000000"
API = "2023-07-01"


class _Bearer(SansIOHTTPPolicy):
    # azure-core refuses bearer tokens over plain http; this sets the header directly.
    def on_request(self, request):
        request.http_request.headers["Authorization"] = "Bearer e30.eyJvaWQiOiJ0ZXN0In0."


class _UnusedCredential:
    def get_token(self, *args, **kwargs):
        raise RuntimeError("not used")


@pytest.fixture
def client(mcle):
    return ResourceManagementClient(_UnusedCredential(), SUB, base_url=mcle, authentication_policy=_Bearer())


def test_resource_groups_and_generic_resources(client):
    g = client.resource_groups.create_or_update("rg1", {"location": "eastus", "tags": {"a": "b"}})
    assert g.id == f"/subscriptions/{SUB}/resourceGroups/rg1"
    assert g.properties.provisioning_state == "Succeeded"
    assert client.resource_groups.check_existence("rg1") is True
    assert client.resource_groups.check_existence("missing") is False
    assert "rg1" in [x.name for x in client.resource_groups.list()]

    r = client.resources.begin_create_or_update(
        "rg1", "Microsoft.KeyVault", "", "vaults", "kv1",
        {"location": "eastus", "properties": {"sku": {"name": "standard"}}}, api_version=API).result()
    assert r.id == f"/subscriptions/{SUB}/resourceGroups/rg1/providers/Microsoft.KeyVault/vaults/kv1"
    assert r.type == "Microsoft.KeyVault/vaults"
    assert r.properties["sku"] == {"name": "standard"}
    assert client.resources.get("rg1", "Microsoft.KeyVault", "", "vaults", "kv1", api_version=API).name == "kv1"
    assert [x.name for x in client.resources.list_by_resource_group("rg1")] == ["kv1"]

    client.resource_groups.begin_delete("rg1").result()
    with pytest.raises(ResourceNotFoundError) as e:
        client.resource_groups.get("rg1")
    assert e.value.error.code == "ResourceGroupNotFound"
