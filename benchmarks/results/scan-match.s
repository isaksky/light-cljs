__RNvCsaCpq0navtZb_9tokenizer10scan_match:
	.cfi_startproc
	cbz	x1, LBB18_19
	sub	sp, sp, #32
	.cfi_def_cfa_offset 32
	mov	x8, x0
	mov	x0, #0
	mov	x12, #0
	add	x9, sp, #8
	mov	x10, #20895015895040
LBB18_2:
	mov	x11, x12
	b	LBB18_4
LBB18_3:
	add	x11, x11, #1
	cmp	x1, x11
	b.eq	LBB18_18
LBB18_4:
	ldrb	w13, [x8, x11]
	sub	w12, w13, #9
	cmp	w12, #5
	b.lo	LBB18_3
	cmp	w13, #44
	b.eq	LBB18_3
	cmp	w13, #32
	b.eq	LBB18_3
	add	x12, x11, #1
	cmp	w13, #41
	b.eq	LBB18_10
	cmp	w13, #40
	b.ne	LBB18_11
	mov	w13, #0
	stp	x11, x12, [sp, #8]
	strb	w13, [sp, #24]
	; InlineAsm Start
	; InlineAsm End
	add	x0, x0, #1
	cmp	x12, x1
	b.lo	LBB18_2
	b	LBB18_18
LBB18_10:
	mov	w13, #1
	stp	x11, x12, [sp, #8]
	strb	w13, [sp, #24]
	; InlineAsm Start
	; InlineAsm End
	add	x0, x0, #1
	cmp	x12, x1
	b.lo	LBB18_2
	b	LBB18_18
LBB18_11:
	cmp	x12, x1
	b.lo	LBB18_13
	b	LBB18_16
LBB18_12:
	add	x12, x12, #1
	mov	w13, #2
	cmp	x1, x12
	b.eq	LBB18_17
LBB18_13:
	ldrb	w13, [x8, x12]
	sub	w14, w13, #9
	cmp	w14, #5
	b.lo	LBB18_16
	cmp	w13, #44
	b.hi	LBB18_12
	lsr	x13, x10, x13
	tbz	w13, #0, LBB18_12
LBB18_16:
	mov	w13, #2
	stp	x11, x12, [sp, #8]
	strb	w13, [sp, #24]
	; InlineAsm Start
	; InlineAsm End
	add	x0, x0, #1
	cmp	x12, x1
	b.lo	LBB18_2
	b	LBB18_18
LBB18_17:
	mov	x12, x1
	stp	x11, x1, [sp, #8]
	strb	w13, [sp, #24]
	; InlineAsm Start
	; InlineAsm End
	add	x0, x0, #1
	cmp	x1, x1
	b.lo	LBB18_2
LBB18_18:
	add	sp, sp, #32
	.cfi_def_cfa_offset 0
	ret
LBB18_19:
	mov	x0, #0
	ret
	.cfi_endproc
