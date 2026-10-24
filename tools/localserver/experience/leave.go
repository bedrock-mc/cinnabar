package experience

// commitLeave applies only private state, with its original scope and revision checks.
// The dispatcher's single worker serializes these writes with its other callbacks.
func (h *Host) commitLeave(exp string, ev event, snap snapshot, ops []Op) error {
	for _, op := range ops {
		if op.StoreWrite == nil {
			return errInvalid
		}
	}
	context := snap.req.Info.Context
	if context == nil {
		return errInvalid
	}
	if !h.store.statesCurrent(exp, stateAddresses(ev.instance.handle.ID, ev.storePrincipal(nil)), context.Stores) {
		return errStale
	}
	plan, err := h.validate(exp, ev, snap, ops)
	if err != nil {
		return err
	}
	for _, grow := range []bool{false, true} {
		for _, write := range plan.states {
			if (write.delta > 0) != grow {
				continue
			}
			if err := h.store.setState(exp, write.address, write.key, write.data, write.present); err != nil {
				return err
			}
		}
	}
	return nil
}
