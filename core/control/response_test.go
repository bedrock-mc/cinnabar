package control

import "testing"

func TestEncodedLenTreatsEncodingFailureAsOverBudget(t *testing.T) {
	if encodedLen(make(chan int)) <= frameBudget {
		t.Fatal("an unencodable result must never be admitted within the frame budget")
	}
}
