module github.com/hashimthearab/rust-mcbe/core

go 1.26.0

require github.com/sandertv/gophertunnel v1.57.0

replace (
	github.com/df-mc/go-nethernet => github.com/HashimTheArab/go-nethernet v0.0.0-20260621185016-ac3e14475524
	github.com/df-mc/go-xsapi/v2 => github.com/HashimTheArab/go-xsapi/v2 v2.0.0-20260620084425-d4a68a0fa178
	github.com/sandertv/go-raknet => github.com/hashimthearab/go-raknet v1.14.2-0.20260625072737-109968c5e6ff
	github.com/sandertv/gophertunnel => github.com/hashimthearab/gophertunnel v1.25.3-0.20260707023624-0635cd9a2ee8
)
