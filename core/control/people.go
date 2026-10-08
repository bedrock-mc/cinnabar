package control

import (
	"context"

	"github.com/hashimthearab/rust-mcbe/core/catalog"
)

// methodFriendsPeople lists the account's Xbox friends for the invite screen.
const methodFriendsPeople = "friends_people.v1"

// PeopleServices lists the account's Xbox friends; a Services value may implement it.
type PeopleServices interface {
	People(ctx context.Context) ([]catalog.Person, error)
}

type peopleResultV1 struct {
	SchemaVersion uint32           `json:"schema_version"`
	Friends       []catalog.Person `json:"friends"`
}

// peopleResult caps the list, then drops trailing friends until the reply fits one control frame.
func peopleResult(people []catalog.Person) peopleResultV1 {
	result := peopleResultV1{SchemaVersion: 1, Friends: people[:min(len(people), catalog.MaxPeople)]}
	if result.Friends == nil {
		result.Friends = []catalog.Person{}
	}
	for len(result.Friends) > 0 && encodedLen(result) > frameBudget {
		result.Friends = result.Friends[:len(result.Friends)-1]
	}
	return result
}
