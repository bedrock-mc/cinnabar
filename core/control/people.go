package control

import "context"

// methodFriendsPeople lists the account's Xbox friends for the invite screen.
const methodFriendsPeople = "friends_people.v1"

// PeopleServices lists the account's Xbox friends, already ordered and bounded for the invite
// screen; a Services value may implement it.
type PeopleServices interface {
	People(ctx context.Context) ([]Person, error)
}

// Person is one Xbox friend as the invite screen lists them; the client caches Gamerpic.URL.
type Person struct {
	XUID     string `json:"xuid"`
	Gamertag string `json:"gamertag"`
	Online   bool   `json:"online"`
	Gamerpic Image  `json:"gamerpic"`
}

type peopleResultV1 struct {
	SchemaVersion uint32   `json:"schema_version"`
	Friends       []Person `json:"friends"`
}

// peopleResult drops trailing friends until the reply fits one control frame.
func peopleResult(people []Person) peopleResultV1 {
	result := peopleResultV1{SchemaVersion: 1, Friends: people}
	if result.Friends == nil {
		result.Friends = []Person{}
	}
	for len(result.Friends) > 0 && encodedLen(result) > frameBudget {
		result.Friends = result.Friends[:len(result.Friends)-1]
	}
	return result
}
