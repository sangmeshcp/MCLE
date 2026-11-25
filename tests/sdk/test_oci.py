# verifies(L1): oci/objectstorage: GetNamespace, CreateBucket, GetBucket, ListBuckets, UpdateBucket, DeleteBucket
import oci
import pytest
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import rsa

FINGERPRINT = "aa:bb:cc:dd:ee:ff:00:11:22:33:44:55:66:77:88:99"


@pytest.fixture
def client(mcle, tmp_path):
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    key_file = tmp_path / "key.pem"
    key_file.write_bytes(key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.TraditionalOpenSSL,
                                           serialization.NoEncryption()))
    cfg = {"region": "us-ashburn-1", "tenancy": "ocid1.tenancy.oc1..t", "user": "ocid1.user.oc1..u",
           "fingerprint": FINGERPRINT, "key_file": str(key_file)}
    signer = oci.signer.Signer(cfg["tenancy"], cfg["user"], FINGERPRINT, str(key_file))
    return oci.object_storage.ObjectStorageClient(cfg, signer=signer, service_endpoint=mcle)


def test_bucket_lifecycle(client):
    models = oci.object_storage.models
    ns = client.get_namespace().data
    comp = "ocid1.compartment.oc1..c"
    b = client.create_bucket(ns, models.CreateBucketDetails(name="b1", compartment_id=comp,
                                                            freeform_tags={"a": "b"})).data
    assert b.name == "b1" and b.namespace == ns and b.id.startswith("ocid1.bucket.")
    assert b.storage_tier == "Standard" and b.time_created is not None

    assert client.get_bucket(ns, "b1").data.freeform_tags == {"a": "b"}
    assert [x.name for x in client.list_buckets(ns, comp).data] == ["b1"]
    assert client.list_buckets(ns, "ocid1.compartment.oc1..other").data == []
    assert client.update_bucket(ns, "b1", models.UpdateBucketDetails(freeform_tags={"x": "y"})).data.freeform_tags == {"x": "y"}

    with pytest.raises(oci.exceptions.ServiceError) as e:
        client.create_bucket(ns, models.CreateBucketDetails(name="b1", compartment_id=comp))
    assert (e.value.status, e.value.code) == (409, "BucketAlreadyExists")

    client.delete_bucket(ns, "b1")
    with pytest.raises(oci.exceptions.ServiceError) as e:
        client.get_bucket(ns, "b1")
    assert (e.value.status, e.value.code) == (404, "BucketNotFound")
