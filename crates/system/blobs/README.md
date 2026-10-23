# blobs

blobs records each bundle of blobs an account puts, by id, length and kind, and refuses
an entry for a blob a program already put (`blob_put`) whose kind or framed length is
another, which no validator would receipt; a current validator receipts a bundle once
its node holds every blob, and two thirds of the current validators make it held, which
is when the host serves its blobs to a program.
