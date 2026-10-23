// Generated from crates/server-experience/schema/extension.json; do not edit.
package cinnabarext

// SignedDocument is the schema-defined SignedDocument document.
type SignedDocument struct {
	Payload   string
	Signature string
}

var signedDocumentFields = []string{"payload", "signature"}

// appendJSON writes SignedDocument in canonical field order.
func (v SignedDocument) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: signedDocumentFields}
	o.str(v.Payload)
	o.str(v.Signature)
	return o.end()
}

// decodeJSON reads a strict SignedDocument object.
func (v *SignedDocument) decodeJSON(r *reader) error {
	*v = SignedDocument{}
	return r.fields(signedDocumentFields, func(name string) (err error) {
		switch name {
		case "payload":
			v.Payload, err = r.str()
		case "signature":
			v.Signature, err = r.str()
		}
		return err
	})
}

// signingBytes builds the domain-separated bytes consumed by native Ed25519 signing.
func signingBytes(domain string, payload []byte) []byte { return append([]byte(domain), payload...) }
