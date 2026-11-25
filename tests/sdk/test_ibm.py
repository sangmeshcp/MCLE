# verifies(L1): ibm/resourcecontroller: create_resource_instance, get_resource_instance, update_resource_instance, list_resource_instances, delete_resource_instance
# verifies(L2): ibm/iamidentity: get_token
import pytest
from ibm_cloud_sdk_core import ApiException
from ibm_cloud_sdk_core.authenticators import IAMAuthenticator
from ibm_platform_services import ResourceControllerV2


def test_resource_instance_lifecycle(mcle):
    rc = ResourceControllerV2(authenticator=IAMAuthenticator("any-api-key", url=mcle))
    rc.set_service_url(mcle)
    r = rc.create_resource_instance(name="inst1", target="us-south", resource_group="rg1",
                                    resource_plan_id="plan-x").get_result()
    assert r["id"].startswith("crn:v1:bluemix:public:") and r["state"] == "active"

    assert rc.get_resource_instance(id=r["guid"]).get_result()["name"] == "inst1"
    assert rc.get_resource_instance(id=r["id"]).get_result()["guid"] == r["guid"]
    assert rc.update_resource_instance(id=r["guid"], name="inst2").get_result()["name"] == "inst2"
    assert "inst2" in [x["name"] for x in rc.list_resource_instances().get_result()["resources"]]

    rc.delete_resource_instance(id=r["guid"])
    with pytest.raises(ApiException) as e:
        rc.get_resource_instance(id=r["guid"])
    assert e.value.status_code == 404
