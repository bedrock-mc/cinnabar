package cinnabarext

// Scalar is one value of a client-channel record, in the form the client part's wire protocol
// gives it: a leaf, or on wire v2 a list or record of values. Exactly one field is set.
type Scalar struct {
	Bool    *bool
	Integer *int64
	Text    *string
	Choice  *uint16
	// List holds the items of a list field, all of its one item type.
	List *[]Scalar
	// Record holds one value per field of a record field, in order.
	Record *[]Scalar
}

// MarshalJSON writes the canonical wire value without HTML escaping.
func (s Scalar) MarshalJSON() ([]byte, error) { return appendScalar(nil, s) }

// UnmarshalJSON validates and reads exactly one canonical wire value.
func (s *Scalar) UnmarshalJSON(data []byte) error {
	return within(data, func(r *reader) error {
		value, err := readScalar(r)
		if err == nil {
			*s = value
		}
		return err
	})
}
