package proxy

import (
	"context"

	"github.com/sandertv/gophertunnel/minecraft"
)

// grantLocalHostOnDial runs only in the ordinary local-client proxy, after canonical upstream identity is known.
func grantLocalHostOnDial(
	dial func(context.Context, *resolvedUpstreamTarget, minecraft.Dialer) (upstreamSession, error),
	grant func(context.Context, string, string) error,
) func(context.Context, *resolvedUpstreamTarget, minecraft.Dialer) (upstreamSession, error) {
	return func(ctx context.Context, target *resolvedUpstreamTarget, dialer minecraft.Dialer) (upstreamSession, error) {
		upstream, err := dial(ctx, target, dialer)
		if err == nil && grant != nil && target.managedWorldAddress != "" {
			err = grant(ctx, target.managedWorldAddress, upstream.IdentityData().DisplayName)
		}
		return upstream, err
	}
}
